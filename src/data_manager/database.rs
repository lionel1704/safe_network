// Copyright 2015 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under (1) the MaidSafe.net Commercial License,
// version 1.0 or later, or (2) The General Public License (GPL), version 3, depending on which
// licence you accepted on initial access to the Software (the "Licences").
//
// By contributing code to the SAFE Network Software, or to this project generally, you agree to be
// bound by the terms of the MaidSafe Contributor Agreement, version 1.0.  This, along with the
// Licenses can be found in the root directory of this project at LICENSE, COPYING and CONTRIBUTOR.
//
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied.
//
// Please review the Licences for the specific language governing permissions and limitations
// relating to use of the SAFE Network Software.

#![allow(dead_code)]

use routing;
use std::collections::HashMap;
use cbor;
use rustc_serialize::{Encodable};

type Identity = routing::NameType; // name of the chunk
use routing::types::PmidNode;
use routing::types::PmidNodes;
use routing::NameType;
use std::cmp;
use routing::generic_sendable_type::GenericSendableType;
use routing::node_interface::RoutingNodeAction;

pub struct DataManagerDatabase {
  storage : HashMap<Identity, PmidNodes>
}

impl DataManagerDatabase {
    pub fn new () -> DataManagerDatabase {
        DataManagerDatabase { storage: HashMap::with_capacity(10000) }
    }

    pub fn exist(&mut self, name : &Identity) -> bool {
        self.storage.contains_key(name)
    }

    pub fn put_pmid_nodes(&mut self, name : &Identity, pmid_nodes: PmidNodes) {
        self.storage.entry(name.clone()).or_insert(pmid_nodes.clone());
    }

    pub fn add_pmid_node(&mut self, name : &Identity, pmid_node: PmidNode) {
        let nodes = self.storage.entry(name.clone()).or_insert(vec![pmid_node.clone()]);
        if !nodes.contains(&pmid_node) {
            nodes.push(pmid_node);
        }
    }

    pub fn remove_pmid_node(&mut self, name : &Identity, pmid_node: PmidNode) {
        if !self.storage.contains_key(name) {
            return;
        }
        let nodes = self.storage.entry(name.clone()).or_insert(vec![]);
        for i in 0..nodes.len() {
            if nodes[i] == pmid_node {
              nodes.remove(i);
              break;
            }
        }
    }

    pub fn get_pmid_nodes(&mut self, name : &Identity) -> PmidNodes {
        let entry = self.storage.get(&name);
        if entry.is_some() {
            entry.unwrap().clone()
        } else {
            Vec::<PmidNode>::new()
        }
    }

    pub fn retrieve_all_and_reset(&mut self, close_group: &mut Vec<NameType>) -> Vec<RoutingNodeAction> {
        for it in self.storage.iter_mut() {
            let mut new_pmid_nodes = Vec::<NameType>::with_capacity(it.1.len());
            for vec_it in it.1.iter() {
                if close_group.iter().find(|a| **a == *vec_it).is_some() {
                    new_pmid_nodes.push(vec_it.clone());
                }
            }

            if new_pmid_nodes.len() < 3 {
                assert!(close_group.len() >= 3);

                close_group.sort_by(|a, b| {
                    if routing::closer_to_target(&a, &b, &it.0) {
                      cmp::Ordering::Less
                    } else {
                      cmp::Ordering::Greater
                    }
                });

                while new_pmid_nodes.len() < 3 {
                    for close_grp_it in close_group.iter() {
                        if new_pmid_nodes.iter().find(|a| **a == *close_grp_it).is_none() {
                            new_pmid_nodes.push(close_grp_it.clone());
                        }
                    }
                }
            }

            *it.1 = new_pmid_nodes;
        }

        let data: Vec<_> = self.storage.drain().collect();
        let mut actions = Vec::<RoutingNodeAction>::new();
        for element in data {
            let mut e = cbor::Encoder::from_memory();
            e.encode(&[&element.1]).unwrap();
            let serialised_content = e.into_bytes();
            let content = GenericSendableType::new(element.0.clone(), 1, serialised_content); //TODO Get type_tag correct
            for destination in close_group.iter() {
                actions.push(RoutingNodeAction::Put{destination: destination.clone(),
                                                    content:     content.clone()});
            }
        }
        actions
    }
}

#[cfg(test)]
mod test {
  use super::*;
  use maidsafe_types::ImmutableData;
  use routing::NameType;
  use routing::types::{generate_random_vec_u8};
  use routing::test_utils::Random;
  use routing::sendable::Sendable;

  #[test]
  fn exist() {
    let mut db = DataManagerDatabase::new();
    let value = generate_random_vec_u8(1024);
    let data = ImmutableData::new(value);
    let mut pmid_nodes : Vec<NameType> = vec![];

    for _ in 0..4 {
      pmid_nodes.push(Random::generate_random());
    }

    let data_name = data.name();
    assert_eq!(db.exist(&data_name), false);
    db.put_pmid_nodes(&data_name, pmid_nodes);
    assert_eq!(db.exist(&data_name), true);
  }

  #[test]
  fn put() {
    let mut db = DataManagerDatabase::new();
    let value = generate_random_vec_u8(1024);
    let data = ImmutableData::new(value);
    let data_name = data.name();
    let mut pmid_nodes : Vec<NameType> = vec![];

    for _ in 0..4 {
      pmid_nodes.push(Random::generate_random());
    }

    let result = db.get_pmid_nodes(&data_name);
    assert_eq!(result.len(), 0);

    db.put_pmid_nodes(&data_name, pmid_nodes.clone());

    let result = db.get_pmid_nodes(&data_name);
    assert_eq!(result.len(), pmid_nodes.len());
  }

  #[test]
  fn remove_pmid() {
    let mut db = DataManagerDatabase::new();
    let value = generate_random_vec_u8(1024);
    let data = ImmutableData::new(value);
    let data_name = data.name();
    let mut pmid_nodes : Vec<NameType> = vec![];

    for _ in 0..4 {
      pmid_nodes.push(Random::generate_random());
    }

    db.put_pmid_nodes(&data_name, pmid_nodes.clone());
    let result = db.get_pmid_nodes(&data_name);
    assert_eq!(result, pmid_nodes);

    db.remove_pmid_node(&data_name, pmid_nodes[0].clone());

    let result = db.get_pmid_nodes(&data_name);
    assert_eq!(result.len(), 3);
    for index in 0..result.len() {
      assert!(result[index] != pmid_nodes[0]);
    }
  }

  #[test]
  fn replace_pmids() {
    let mut db = DataManagerDatabase::new();
    let value = generate_random_vec_u8(1024);
    let data = ImmutableData::new(value);
    let data_name = data.name();
    let mut pmid_nodes : Vec<NameType> = vec![];
    let mut new_pmid_nodes : Vec<NameType> = vec![];

    for _ in 0..4 {
      pmid_nodes.push(Random::generate_random());
      new_pmid_nodes.push(Random::generate_random());
    }

    db.put_pmid_nodes(&data_name, pmid_nodes.clone());
    let result = db.get_pmid_nodes(&data_name);
    assert_eq!(result, pmid_nodes);
    assert!(result != new_pmid_nodes);

    for index in 0..4 {
      db.remove_pmid_node(&data_name, pmid_nodes[index].clone());
      db.add_pmid_node(&data_name, new_pmid_nodes[index].clone());
    }

    let result = db.get_pmid_nodes(&data_name);
    assert_eq!(result, new_pmid_nodes);
    assert!(result != pmid_nodes);
  }
}
