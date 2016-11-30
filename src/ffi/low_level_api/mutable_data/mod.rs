// Copyright 2016 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under (1) the MaidSafe.net
// Commercial License, version 1.0 or later, or (2) The General Public License
// (GPL), version 3, depending on which licence you accepted on initial access
// to the Software (the "Licences").
//
// By contributing code to the SAFE Network Software, or to this project
// generally, you agree to be bound by the terms of the MaidSafe Contributor
// Agreement, version 1.0.
// This, along with the Licenses can be found in the root directory of this
// project at LICENSE, COPYING and CONTRIBUTOR.
//
// Unless required by applicable law or agreed to in writing, the SAFE Network
// Software distributed under the GPL Licence is distributed on an "AS IS"
// BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or
// implied.
//
// Please review the Licences for the specific language governing permissions
// and limitations relating to use of the SAFE Network Software.

pub mod entry_actions;
pub mod entries;
pub mod permissions;
mod helper;

use core::{CoreError, FutureExt};
use ffi::{MDataEntriesHandle, MDataEntryActionsHandle, MDataKeysHandle, MDataPermissionSetHandle,
          MDataPermissionsHandle, MDataValuesHandle, OpaqueCtx, Session, SignKeyHandle};
use ffi::errors::FfiError;
use ffi::helper as ffi_helper;
use ffi::object_cache::ObjectCache;
use futures::Future;
use routing::{MutableData, User, XOR_NAME_LEN, XorName};
use std::os::raw::c_void;

/// Create new mutable data and put it on the network.
///
/// `permissions_h` is a handle to permissions to be set on the mutable data.
/// If 0, the permissions will be empty.
/// `entries_h` is a handle to entries for the mutable data. If 0, the entries will be empty.
#[no_mangle]
pub unsafe extern "C" fn mdata_put(session: *const Session,
                                   name: *const [u8; XOR_NAME_LEN],
                                   type_tag: u64,
                                   permissions_h: MDataPermissionsHandle,
                                   entries_h: MDataEntriesHandle,
                                   user_data: *mut c_void,
                                   o_cb: unsafe extern "C" fn(*mut c_void, i32)) {
    ffi_helper::catch_unwind_cb(user_data, o_cb, || {
        let user_data = OpaqueCtx(user_data);
        let name = XorName(*name);

        (*session).send(move |client, object_cache| {
            let sign_pk = try_cb!(client.public_signing_key(), user_data, o_cb);

            let permissions = if permissions_h != 0 {
                try_cb!(object_cache.get_mdata_permissions(permissions_h),
                        user_data,
                        o_cb)
                    .clone()
            } else {
                Default::default()
            };

            let entries = if entries_h != 0 {
                try_cb!(object_cache.get_mdata_entries(entries_h), user_data, o_cb).clone()
            } else {
                Default::default()
            };

            let data = try_cb!(MutableData::new(name,
                                                type_tag,
                                                permissions,
                                                entries,
                                                btree_set![sign_pk])
                                   .map_err(CoreError::from),
                               user_data,
                               o_cb);

            client.put_mdata(data, None)
                .then(move |result| {
                    o_cb(user_data.0, ffi_result_code!(result));
                    Ok(())
                })
                .into_box()
                .into()
        })
    })
}

/// Get version of the mutable data.
#[no_mangle]
pub unsafe extern "C" fn mdata_get_version(session: *const Session,
                                           name: *const [u8; XOR_NAME_LEN],
                                           type_tag: u64,
                                           user_data: *mut c_void,
                                           o_cb: unsafe extern "C" fn(*mut c_void, i32, u64)) {
    ffi_helper::catch_unwind_cb(user_data, o_cb, || {
        let name = XorName(*name);

        helper::send_async(session,
                           user_data,
                           o_cb,
                           move |client, _| client.get_mdata_version(name, type_tag, None))
    })
}

/// Get value at the given key from the mutable data.
/// The arguments to the callback are:
///     1. user data
///     2. error code
///     3. pointer to content
///     4. content length
///     5. content capacity
///     6. entry version
#[no_mangle]
pub unsafe extern "C" fn mdata_get_value(session: *const Session,
                                         name: *const [u8; XOR_NAME_LEN],
                                         type_tag: u64,
                                         key_ptr: *const u8,
                                         key_len: usize,
                                         user_data: *mut c_void,
                                         o_cb: unsafe extern "C" fn(*mut c_void,
                                                                    i32,
                                                                    *mut u8,
                                                                    usize,
                                                                    usize,
                                                                    u64)) {
    ffi_helper::catch_unwind_cb(user_data, o_cb, || {
        let name = XorName(*name);
        let key = ffi_helper::u8_ptr_to_vec(key_ptr, key_len);

        helper::send_async(session, user_data, o_cb, move |client, _| {
            client.get_mdata_value(name, type_tag, key, None)
                .map(move |value| {
                    let content = ffi_helper::u8_vec_to_ptr(value.content);
                    (content.0, content.1, content.2, value.entry_version)
                })
        })
    })
}

/// Get complete list of entries in the mutable data.
#[no_mangle]
pub unsafe extern "C" fn mdata_list_entries(session: *const Session,
                                            name: *const [u8; XOR_NAME_LEN],
                                            type_tag: u64,
                                            user_data: *mut c_void,
                                            o_cb: unsafe extern "C" fn(*mut c_void,
                                                                       i32,
                                                                       MDataEntriesHandle)) {
    ffi_helper::catch_unwind_cb(user_data, o_cb, || {
        let name = XorName(*name);

        helper::send_async(session, user_data, o_cb, move |client, object_cache| {
            let object_cache = object_cache.clone();
            client.list_mdata_entries(name, type_tag, None)
                .map(move |entries| object_cache.insert_mdata_entries(entries))
        })
    })
}

/// Get list of keys in the mutable data.
#[no_mangle]
pub unsafe extern "C" fn mdata_list_keys(session: *const Session,
                                         name: *const [u8; XOR_NAME_LEN],
                                         type_tag: u64,
                                         user_data: *mut c_void,
                                         o_cb: unsafe extern "C" fn(*mut c_void,
                                                                    i32,
                                                                    MDataKeysHandle)) {
    ffi_helper::catch_unwind_cb(user_data, o_cb, || {
        let name = XorName(*name);

        helper::send_async(session, user_data, o_cb, move |client, object_cache| {
            let object_cache = object_cache.clone();
            client.list_mdata_keys(name, type_tag, None)
                .map(move |keys| object_cache.insert_mdata_keys(keys))
        })
    })
}

/// Get list of values in the mutable data.
#[no_mangle]
pub unsafe extern "C" fn mdata_list_values(session: *const Session,
                                           name: *const [u8; XOR_NAME_LEN],
                                           type_tag: u64,
                                           user_data: *mut c_void,
                                           o_cb: unsafe extern "C" fn(*mut c_void,
                                                                      i32,
                                                                      MDataValuesHandle)) {
    ffi_helper::catch_unwind_cb(user_data, o_cb, || {
        let name = XorName(*name);

        helper::send_async(session, user_data, o_cb, move |client, object_cache| {
            let object_cache = object_cache.clone();
            client.list_mdata_values(name, type_tag, None)
                .map(move |values| object_cache.insert_mdata_values(values))
        })
    })
}

/// Mutate entries of the mutable data.
#[no_mangle]
pub unsafe fn mdata_mutate_entries(session: *const Session,
                                   name: *const [u8; XOR_NAME_LEN],
                                   type_tag: u64,
                                   actions_h: MDataEntryActionsHandle,
                                   user_data: *mut c_void,
                                   o_cb: unsafe extern "C" fn(*mut c_void, i32)) {
    ffi_helper::catch_unwind_cb(user_data, o_cb, || {
        let user_data = OpaqueCtx(user_data);
        let name = XorName(*name);

        (*session).send(move |client, object_cache| {
            let actions = try_cb!(object_cache.get_mdata_entry_actions(actions_h),
                                  user_data,
                                  o_cb)
                .clone();

            client.mutate_mdata_entries(name, type_tag, actions, None)
                .then(move |result| {
                    o_cb(user_data.0, ffi_result_code!(result));
                    Ok(())
                })
                .into_box()
                .into()
        })
    })
}

/// Get list of all permissions set on the mutable data
#[no_mangle]
pub unsafe fn mdata_list_permissions(session: *const Session,
                                     name: *const [u8; XOR_NAME_LEN],
                                     type_tag: u64,
                                     user_data: *mut c_void,
                                     o_cb: unsafe extern "C" fn(*mut c_void,
                                                                i32,
                                                                MDataPermissionsHandle)) {
    ffi_helper::catch_unwind_cb(user_data, o_cb, || {
        let name = XorName(*name);

        helper::send_async(session, user_data, o_cb, move |client, object_cache| {
            let object_cache = object_cache.clone();
            client.list_mdata_permissions(name, type_tag, None)
                .map(move |perms| object_cache.insert_mdata_permissions(perms))
        })
    })
}

/// Get list of permissions set on the mutable data for the given user.
///
/// User is either handle to a signing key, or 0 which means "anyone".
#[no_mangle]
pub unsafe fn mdata_list_user_permissions(session: *const Session,
                                          name: *const [u8; XOR_NAME_LEN],
                                          type_tag: u64,
                                          user_h: SignKeyHandle,
                                          user_data: *mut c_void,
                                          o_cb: unsafe extern "C" fn(*mut c_void,
                                                                     i32,
                                                                     MDataPermissionSetHandle)) {
    ffi_helper::catch_unwind_cb(user_data, o_cb, || {
        let user_data = OpaqueCtx(user_data);
        let name = XorName(*name);

        (*session).send(move |client, object_cache| {
            let object_cache = object_cache.clone();
            let user = try_cb!(get_user(&object_cache, user_h), user_data, o_cb);

            client.list_mdata_user_permissions(name, type_tag, user, None)
                .map(move |set| {
                    let handle = object_cache.insert_mdata_permission_set(set);
                    o_cb(user_data.0, 0, handle);
                })
                .map_err(move |err| o_cb(user_data.0, ffi_error_code!(err), 0))
                .into_box()
                .into()
        })
    })
}

/// Set permissions set on the mutable data for the given user.
///
/// User is either handle to a signing key, or 0 which means "anyone".
#[no_mangle]
pub unsafe fn mdata_set_user_permissions(session: *const Session,
                                         name: *const [u8; XOR_NAME_LEN],
                                         type_tag: u64,
                                         user_h: SignKeyHandle,
                                         permission_set_h: MDataPermissionSetHandle,
                                         version: u64,
                                         user_data: *mut c_void,
                                         o_cb: unsafe extern "C" fn(*mut c_void, i32)) {
    ffi_helper::catch_unwind_cb(user_data, o_cb, || {
        let user_data = OpaqueCtx(user_data);
        let name = XorName(*name);

        (*session).send(move |client, object_cache| {
            let user = try_cb!(get_user(object_cache, user_h), user_data, o_cb);
            let permission_set = try_cb!(object_cache.get_mdata_permission_set(permission_set_h),
                                         user_data,
                                         o_cb)
                .clone();

            client.set_mdata_user_permissions(name, type_tag, user, permission_set, version, None)
                .then(move |result| {
                    o_cb(user_data.0, ffi_result_code!(result));
                    Ok(())
                })
                .into_box()
                .into()
        })
    })
}

/// Delete permissions set on the mutable data for the given user.
///
/// User is either handle to a signing key, or 0 which means "anyone".
#[no_mangle]
pub unsafe fn mdata_del_user_permissions(session: *const Session,
                                         name: *const [u8; XOR_NAME_LEN],
                                         type_tag: u64,
                                         user_h: SignKeyHandle,
                                         version: u64,
                                         user_data: *mut c_void,
                                         o_cb: unsafe extern "C" fn(*mut c_void, i32)) {
    ffi_helper::catch_unwind_cb(user_data, o_cb, || {
        let user_data = OpaqueCtx(user_data);
        let name = XorName(*name);

        (*session).send(move |client, object_cache| {
            let user = try_cb!(get_user(object_cache, user_h), user_data, o_cb);

            client.del_mdata_user_permissions(name, type_tag, user, version, None)
                .then(move |result| {
                    o_cb(user_data.0, ffi_result_code!(result));
                    Ok(())
                })
                .into_box()
                .into()
        })
    })
}

/// Change owner of the mutable data.
#[no_mangle]
pub unsafe extern "C" fn mdata_change_owner(session: *const Session,
                                            name: *const [u8; XOR_NAME_LEN],
                                            type_tag: u64,
                                            new_owner_h: SignKeyHandle,
                                            version: u64,
                                            user_data: *mut c_void,
                                            o_cb: unsafe extern "C" fn(*mut c_void, i32)) {
    ffi_helper::catch_unwind_cb(user_data, o_cb, || {
        let user_data = OpaqueCtx(user_data);
        let name = XorName(*name);

        (*session).send(move |client, object_cache| {
            let new_owner = *try_cb!(object_cache.get_sign_key(new_owner_h), user_data, o_cb);

            client.change_mdata_owner(name, type_tag, new_owner, version, None)
                .then(move |result| {
                    o_cb(user_data.0, ffi_result_code!(result));
                    Ok(())
                })
                .into_box()
                .into()
        })
    })
}

fn get_user(object_cache: &ObjectCache, handle: SignKeyHandle) -> Result<User, FfiError> {
    let user = if handle != 0 {
        let sign_key = object_cache.get_sign_key(handle)?;
        User::Key(*sign_key)
    } else {
        User::Anyone
    };

    Ok(user)
}
