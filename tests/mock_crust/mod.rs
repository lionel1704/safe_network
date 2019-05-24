// Copyright 2018 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

mod accumulate;
mod cache;
mod churn;
mod client_restrictions;
mod drop;
mod merge;
mod requests;
mod utils;

pub use self::utils::{
    add_connected_nodes_until_one_away_from_split, add_connected_nodes_until_split,
    clear_relocation_overrides, count_sections, create_connected_clients, create_connected_nodes,
    create_connected_nodes_until_split, current_sections, gen_bytes, gen_immutable_data, gen_range,
    gen_range_except, poll_all, poll_and_resend, poll_and_resend_until,
    remove_nodes_which_failed_to_connect, sort_nodes_by_distance_to,
    verify_invariant_for_all_nodes, Nodes, TestClient, TestNode,
};
use fake_clock::FakeClock;
use itertools::Itertools;
use rand::Rng;
use routing::mock_crust::{Endpoint, Network};
use routing::{test_consts, BootstrapConfig, Event, EventStream, Prefix, PublicId, XorName};

pub const MIN_SECTION_SIZE: usize = 3;

// -----  Miscellaneous tests below  -----

fn nodes_with_candidate(nodes: &[TestNode]) -> Vec<XorName> {
    nodes
        .iter()
        .filter(|node| {
            node.inner
                .node_state_unchecked()
                .has_resource_proof_candidate()
        })
        .map(TestNode::name)
        .collect()
}

fn test_nodes(percentage_size: usize) {
    let size = MIN_SECTION_SIZE * percentage_size / 100;
    let network = Network::new(MIN_SECTION_SIZE, None);
    let mut nodes = create_connected_nodes(&network, size);
    verify_invariant_for_all_nodes(&mut nodes);
}

fn nodes_with_prefix_mut<'a>(
    nodes: &'a mut [TestNode],
    prefix: &'a Prefix<XorName>,
) -> impl Iterator<Item = &'a mut TestNode> {
    nodes
        .iter_mut()
        .filter(move |node| prefix.matches(&node.name()))
}

#[test]
fn disconnect_on_rebootstrap() {
    let network = Network::new(MIN_SECTION_SIZE, None);
    let mut nodes = create_connected_nodes(&network, 2);
    // Try to bootstrap to another than the first node. With network size 2, this should fail.
    let bootstrap_config = BootstrapConfig::with_contacts(&[nodes[1].handle.endpoint()]);
    nodes.push(
        TestNode::builder(&network)
            .bootstrap_config(bootstrap_config)
            .endpoint(Endpoint(2))
            .create(),
    );
    let _ = poll_all(&mut nodes, &mut []);
    // When retrying to bootstrap, we should have disconnected from the bootstrap node.
    assert!(!unwrap!(nodes.last()).handle.is_connected(&nodes[1].handle));
    expect_next_event!(unwrap!(nodes.last_mut()), Event::Terminated);
}

#[test]
fn candidate_timeout_resource_proof() {
    let network = Network::new(MIN_SECTION_SIZE, None);
    let mut nodes = create_connected_nodes(&network, MIN_SECTION_SIZE);
    let bootstrap_config = BootstrapConfig::with_contacts(&[nodes[0].handle.endpoint()]);
    nodes.insert(
        0,
        TestNode::builder(&network)
            .bootstrap_config(bootstrap_config)
            .create(),
    );

    // Initiate connection until the candidate switch to ProvingNode:
    info!("Candidate joining name: {}", nodes[0].name());
    poll_and_resend_until(&mut nodes, &mut [], &|nodes| {
        nodes[0].inner.is_proving_node()
    });
    let proving_node = nodes.remove(0);

    assert!(
        proving_node.inner.is_proving_node(),
        "Accepted as candidate"
    );

    // Continue without the joining node until all nodes idle:
    info!("Candidate new name: {}", proving_node.name());
    poll_and_resend(&mut nodes, &mut []);

    assert_eq!(
        nodes.iter().map(TestNode::name).collect_vec(),
        nodes_with_candidate(&nodes),
        "All members of destination section accepted node as candidate"
    );

    // Continue after candidate time out:
    FakeClock::advance_time(1000 * test_consts::CANDIDATE_EXPIRED_TIMEOUT_SECS);
    poll_and_resend(&mut nodes, &mut []);

    assert_eq!(
        Vec::<XorName>::new(),
        nodes_with_candidate(&nodes),
        "All members have rejected the candidate"
    );
}

#[test]
fn single_section() {
    let sec_size = 10;
    let network = Network::new(sec_size, None);
    let mut nodes = create_connected_nodes(&network, sec_size);
    verify_invariant_for_all_nodes(&mut nodes);
}

#[test]
fn less_than_section_size_nodes() {
    test_nodes(80)
}

#[test]
fn equal_section_size_nodes() {
    test_nodes(100);
}

#[test]
fn more_than_section_size_nodes() {
    test_nodes(600);
}

#[test]
fn client_connects_to_nodes() {
    let network = Network::new(MIN_SECTION_SIZE, None);
    let mut nodes = create_connected_nodes(&network, MIN_SECTION_SIZE + 1);
    let _ = create_connected_clients(&network, &mut nodes, 1);
}

#[test]
fn node_joins_in_front() {
    let network = Network::new(MIN_SECTION_SIZE, None);
    let mut nodes = create_connected_nodes(&network, 2 * MIN_SECTION_SIZE);
    let bootstrap_config = BootstrapConfig::with_contacts(&[nodes[0].handle.endpoint()]);
    nodes.insert(
        0,
        TestNode::builder(&network)
            .bootstrap_config(bootstrap_config)
            .create(),
    );
    poll_and_resend(&mut nodes, &mut []);

    verify_invariant_for_all_nodes(&mut nodes);
}

#[test]
fn multiple_joining_nodes() {
    let network = Network::new(MIN_SECTION_SIZE, None);
    let mut nodes = create_connected_nodes(&network, MIN_SECTION_SIZE);
    let bootstrap_config = BootstrapConfig::with_contacts(&[nodes[0].handle.endpoint()]);

    while nodes.len() < 40 {
        info!("Size {}", nodes.len());

        // Try adding five nodes at once, possibly to the same section. This makes sure one section
        // can handle this, either by adding the nodes in sequence or by rejecting some.
        let count = 5;
        for _ in 0..count {
            nodes.push(
                TestNode::builder(&network)
                    .bootstrap_config(bootstrap_config.clone())
                    .create(),
            );
        }

        poll_and_resend(&mut nodes, &mut []);
        let removed_count = remove_nodes_which_failed_to_connect(&mut nodes, count);
        let nodes_added: Vec<_> = nodes
            .iter()
            .rev()
            .take(count - removed_count)
            .map(TestNode::name)
            .collect();
        info!("Added Nodes: {:?}", nodes_added);
        verify_invariant_for_all_nodes(&mut nodes);
        assert!(
            !nodes_added.is_empty(),
            "Should always handle at least one node"
        );
    }
}

#[test]
fn multi_split() {
    let network = Network::new(MIN_SECTION_SIZE, None);
    let mut nodes = create_connected_nodes_until_split(&network, vec![2, 2, 2, 2], false);
    verify_invariant_for_all_nodes(&mut nodes);
}

struct SimultaneousJoiningNode {
    // relocation_dst to set for nodes in the section with src_section_prefix: Must match section.
    dst_section_prefix: Prefix<XorName>,
    // section prefix that will contain the new node to add: Must match section.
    src_section_prefix: Prefix<XorName>,
    // The relocation_interval to set for nodes in the section with dst_section_prefix:
    // Must be within dst_section_prefix. If none let production code decide.
    dst_relocation_interval_prefix: Option<Prefix<XorName>>,
    // The prefix to find the proxy within.
    proxy_prefix: Prefix<XorName>,
}

// Proceed with testing joining nodes at the same time with the given configuration.
fn simultaneous_joining_nodes(
    network: Network<PublicId>,
    mut nodes: Nodes,
    nodes_to_add_setup: &[SimultaneousJoiningNode],
) {
    //
    // Arrange
    //
    let mut rng = network.new_rng();
    rng.shuffle(&mut nodes);

    let mut nodes_to_add = Vec::new();
    for setup in nodes_to_add_setup {
        // Set the specified relocation destination on the nodes of the given prefixes
        let relocation_dst = setup.dst_section_prefix.substituted_in(rng.gen());
        nodes_with_prefix_mut(&mut nodes, &setup.src_section_prefix)
            .for_each(|node| node.inner.set_next_relocation_dst(Some(relocation_dst)));

        // Set the specified relocation interval on the nodes of the given prefixes
        let relocation_interval = setup
            .dst_relocation_interval_prefix
            .map(|prefix| (prefix.lower_bound(), prefix.upper_bound()));
        nodes_with_prefix_mut(&mut nodes, &setup.dst_section_prefix)
            .for_each(|node| node.inner.set_next_relocation_interval(relocation_interval));

        // Create nodes and find proxies from the given prefixes
        let node_to_add = {
            // Get random bootstrap node from within proxy_prefix
            let bootstrap_config = {
                let mut compatible_proxies =
                    nodes_with_prefix_mut(&mut nodes, &setup.proxy_prefix).collect_vec();
                rng.shuffle(&mut compatible_proxies);

                BootstrapConfig::with_contacts(&[unwrap!(nodes.first()).handle.endpoint()])
            };

            // Get random new TestNode from within src_prefix
            loop {
                let node = TestNode::builder(&network)
                    .bootstrap_config(bootstrap_config.clone())
                    .create();
                if setup.src_section_prefix.matches(&node.name()) {
                    break node;
                }
            }
        };
        nodes_to_add.push(node_to_add);
    }

    //
    // Arrange
    // Add new nodes and process until complete
    //
    nodes.extend(nodes_to_add.into_iter());
    poll_and_resend(&mut nodes, &mut []);

    //
    // Assert
    // Verify that the new nodes are now full nodes part of a section and other invariants.
    //
    let non_full_nodes = nodes
        .iter()
        .filter(|node| !node.inner.is_node())
        .map(TestNode::name)
        .collect_vec();
    assert!(
        non_full_nodes.is_empty(),
        "Should be full node: {:?}",
        non_full_nodes
    );
    verify_invariant_for_all_nodes(&mut nodes);
}

#[test]
fn simultaneous_joining_nodes_two_sections() {
    // Create a network with two sections:
    let network = Network::new(MIN_SECTION_SIZE, None);
    let nodes = create_connected_nodes_until_split(&network, vec![1, 1], false);

    let prefix_0 = Prefix::default().pushed(false);
    let prefix_1 = Prefix::default().pushed(true);

    // Relocate in section we were spawned to with a proxy from prefix_0
    let nodes_to_add_setup = vec![
        SimultaneousJoiningNode {
            dst_section_prefix: prefix_0,
            src_section_prefix: prefix_0,
            dst_relocation_interval_prefix: None,
            proxy_prefix: prefix_0,
        },
        SimultaneousJoiningNode {
            dst_section_prefix: prefix_1,
            src_section_prefix: prefix_1,
            dst_relocation_interval_prefix: None,
            proxy_prefix: prefix_0,
        },
    ];
    simultaneous_joining_nodes(network, nodes, &nodes_to_add_setup);
}

#[test]
fn simultaneous_joining_nodes_three_section_with_one_ready_to_split() {
    // TODO: Use same section size once we have a reliable message relay that handle split.
    // Allow for more route hops otherwise NodeApproaval get losts.
    let min_section_size = MIN_SECTION_SIZE + 1;

    // Create a network with three sections:
    let network = Network::new(min_section_size, None);
    let mut nodes = create_connected_nodes_until_split(&network, vec![1, 2, 2], false);

    // The created sections
    let sections = current_sections(&nodes).into_iter().collect_vec();
    let small_prefix = *unwrap!(sections.iter().find(|prefix| prefix.bit_count() == 1));
    let long_prefix_0 = *unwrap!(sections.iter().find(|prefix| prefix.bit_count() == 2));
    let long_prefix_1 = long_prefix_0.sibling();

    // Setup the network so the small_prefix will split with one more node in small_prefix_to_add.
    let small_prefix_to_add = *unwrap!(add_connected_nodes_until_one_away_from_split(
        &network,
        &mut nodes,
        &[small_prefix],
        false
    )
    .first());

    // First node will trigger the split: src, destination and proxy together.
    // Other nodes validate getting relocated to a section with a proxy from section splitting
    // which will no longer be a neighbour after the split.
    let nodes_to_add_setup = vec![
        SimultaneousJoiningNode {
            dst_section_prefix: small_prefix,
            src_section_prefix: small_prefix,
            dst_relocation_interval_prefix: Some(small_prefix_to_add),
            proxy_prefix: small_prefix,
        },
        SimultaneousJoiningNode {
            dst_section_prefix: long_prefix_0,
            src_section_prefix: small_prefix,
            dst_relocation_interval_prefix: Some(long_prefix_0),
            proxy_prefix: long_prefix_0.with_flipped_bit(0).with_flipped_bit(1),
        },
        SimultaneousJoiningNode {
            dst_section_prefix: long_prefix_1,
            src_section_prefix: long_prefix_0,
            dst_relocation_interval_prefix: Some(long_prefix_1),
            proxy_prefix: long_prefix_1.with_flipped_bit(0).with_flipped_bit(1),
        },
    ];
    simultaneous_joining_nodes(network, nodes, &nodes_to_add_setup);
}

#[test]
fn check_close_names_for_min_section_size_nodes() {
    let nodes = create_connected_nodes(&Network::new(MIN_SECTION_SIZE, None), MIN_SECTION_SIZE);
    let close_sections_complete = nodes
        .iter()
        .all(|n| nodes.iter().all(|m| m.close_names().contains(&n.name())));
    assert!(close_sections_complete);
}
