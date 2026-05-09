#![no_std]

use soroban_sdk::{
    contract, contractimpl, contracttype, token, vec, Address, Env, Vec,
};

// ── Storage keys ────────────────────────────────────────────────────────────

#[contracttype]
pub enum DataKey {
    /// Quorum slice: the set of peers a node explicitly trusts (mirrors FBA).
    TrustSlice(Address),
    /// Accumulated funds deposited by a funder, keyed by funder address.
    Deposit(Address),
}

// ── Contract ─────────────────────────────────────────────────────────────────

#[contract]
pub struct TrustFlow;

#[contractimpl]
impl TrustFlow {
    // ── Trust graph ──────────────────────────────────────────────────────────

    /// Declare your quorum slice — the peers you trust.
    /// Analogous to a validator publishing its quorum slice in SCP.
    pub fn set_trust(env: Env, caller: Address, peers: Vec<Address>) {
        caller.require_auth();
        env.storage()
            .persistent()
            .set(&DataKey::TrustSlice(caller), &peers);
    }

    /// Return the quorum slice for `node`, or an empty vec if unset.
    pub fn get_trust(env: Env, node: Address) -> Vec<Address> {
        env.storage()
            .persistent()
            .get(&DataKey::TrustSlice(node))
            .unwrap_or(vec![&env])
    }

    // ── Funding ──────────────────────────────────────────────────────────────

    /// Deposit `amount` of `token_id` into the contract on behalf of `funder`.
    /// The funder must have pre-approved this contract to spend the tokens.
    pub fn deposit(env: Env, funder: Address, token_id: Address, amount: i128) {
        funder.require_auth();
        assert!(amount > 0, "amount must be positive");

        let client = token::Client::new(&env, &token_id);
        client.transfer(&funder, &env.current_contract_address(), &amount);

        let key = DataKey::Deposit(funder.clone());
        let prev: i128 = env.storage().persistent().get(&key).unwrap_or(0);
        env.storage().persistent().set(&key, &(prev + amount));
    }

    /// Distribute `amount` of `token_id` from `funder`'s deposit to every
    /// address reachable within `max_hops` hops in the trust graph, splitting
    /// the total evenly among all reachable recipients.
    ///
    /// This is the core TrustFlow primitive: capital flows along trust edges,
    /// exactly as quorum agreement propagates through overlapping slices in FBA.
    pub fn distribute(
        env: Env,
        funder: Address,
        token_id: Address,
        amount: i128,
        max_hops: u32,
    ) {
        funder.require_auth();
        assert!(amount > 0, "amount must be positive");
        assert!(max_hops > 0 && max_hops <= 5, "max_hops must be 1-5");

        // Deduct from funder's deposit
        let dep_key = DataKey::Deposit(funder.clone());
        let balance: i128 = env
            .storage()
            .persistent()
            .get(&dep_key)
            .expect("no deposit");
        assert!(balance >= amount, "insufficient deposit");
        env.storage().persistent().set(&dep_key, &(balance - amount));

        // BFS over the trust graph up to max_hops
        let recipients = Self::reachable(&env, &funder, max_hops);
        let n = recipients.len() as i128;
        assert!(n > 0, "no reachable peers");

        let share = amount / n;
        let remainder = amount - share * n;

        let token_client = token::Client::new(&env, &token_id);
        for (i, recipient) in recipients.iter().enumerate() {
            let payout = if i == 0 { share + remainder } else { share };
            if payout > 0 {
                token_client.transfer(
                    &env.current_contract_address(),
                    &recipient,
                    &payout,
                );
            }
        }
    }

    /// Return the deposit balance for `funder`.
    pub fn balance(env: Env, funder: Address) -> i128 {
        env.storage()
            .persistent()
            .get(&DataKey::Deposit(funder))
            .unwrap_or(0)
    }

    // ── Internal BFS ─────────────────────────────────────────────────────────

    /// Collect all addresses reachable from `origin` within `max_hops` hops,
    /// excluding `origin` itself. Uses iterative BFS bounded by hop count.
    fn reachable(env: &Env, origin: &Address, max_hops: u32) -> Vec<Address> {
        // frontier: nodes to expand next
        let mut frontier: Vec<Address> = vec![env];
        frontier.push_back(origin.clone());

        // visited: origin + all seen nodes (to avoid cycles)
        let mut visited: Vec<Address> = vec![env];
        visited.push_back(origin.clone());

        // result: reachable non-origin nodes
        let mut result: Vec<Address> = vec![env];

        let mut hop = 0u32;
        while hop < max_hops && !frontier.is_empty() {
            let mut next_frontier: Vec<Address> = vec![env];
            for node in frontier.iter() {
                let peers: Vec<Address> = env
                    .storage()
                    .persistent()
                    .get(&DataKey::TrustSlice(node.clone()))
                    .unwrap_or(vec![env]);
                for peer in peers.iter() {
                    if !Self::vec_contains(&visited, &peer) {
                        visited.push_back(peer.clone());
                        next_frontier.push_back(peer.clone());
                        result.push_back(peer.clone());
                    }
                }
            }
            frontier = next_frontier;
            hop += 1;
        }
        result
    }

    fn vec_contains(v: &Vec<Address>, target: &Address) -> bool {
        for item in v.iter() {
            if item == *target {
                return true;
            }
        }
        false
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod test {
    use super::*;
    use soroban_sdk::{
        testutils::{Address as _, MockAuth, MockAuthInvoke},
        token::{Client as TokenClient, StellarAssetClient},
        Address, Env, IntoVal,
    };

    fn setup() -> (Env, Address, Address) {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register(TrustFlow, ());
        let token_admin = Address::generate(&env);
        let token_id = env.register_stellar_asset_contract_v2(token_admin.clone()).address();
        (env, contract_id, token_id)
    }

    fn mint(env: &Env, token_id: &Address, to: &Address, amount: i128) {
        StellarAssetClient::new(env, token_id).mint(to, &amount);
    }

    #[test]
    fn test_set_and_get_trust() {
        let (env, contract_id, _) = setup();
        let alice = Address::generate(&env);
        let bob = Address::generate(&env);
        let carol = Address::generate(&env);

        let client = TrustFlowClient::new(&env, &contract_id);
        client.set_trust(&alice, &vec![&env, bob.clone(), carol.clone()]);

        let slice = client.get_trust(&alice);
        assert_eq!(slice.len(), 2);
        assert_eq!(slice.get(0).unwrap(), bob);
        assert_eq!(slice.get(1).unwrap(), carol);
    }

    #[test]
    fn test_deposit_and_balance() {
        let (env, contract_id, token_id) = setup();
        let alice = Address::generate(&env);
        mint(&env, &token_id, &alice, 1000);

        let client = TrustFlowClient::new(&env, &contract_id);
        client.deposit(&alice, &token_id, &500);
        assert_eq!(client.balance(&alice), 500);
    }

    #[test]
    fn test_distribute_one_hop() {
        let (env, contract_id, token_id) = setup();
        let alice = Address::generate(&env);
        let bob = Address::generate(&env);
        let carol = Address::generate(&env);

        mint(&env, &token_id, &alice, 1000);

        let client = TrustFlowClient::new(&env, &contract_id);
        // Alice trusts Bob and Carol
        client.set_trust(&alice, &vec![&env, bob.clone(), carol.clone()]);
        client.deposit(&alice, &token_id, &1000);
        client.distribute(&alice, &token_id, &1000, &1);

        let token = TokenClient::new(&env, &token_id);
        assert_eq!(token.balance(&bob), 500);
        assert_eq!(token.balance(&carol), 500);
        assert_eq!(client.balance(&alice), 0);
    }

    #[test]
    fn test_distribute_two_hops() {
        let (env, contract_id, token_id) = setup();
        let alice = Address::generate(&env);
        let bob = Address::generate(&env);
        let carol = Address::generate(&env);
        let dave = Address::generate(&env);

        mint(&env, &token_id, &alice, 900);

        let client = TrustFlowClient::new(&env, &contract_id);
        // Alice -> Bob -> Carol, Dave
        client.set_trust(&alice, &vec![&env, bob.clone()]);
        client.set_trust(&bob, &vec![&env, carol.clone(), dave.clone()]);
        client.deposit(&alice, &token_id, &900);
        client.distribute(&alice, &token_id, &900, &2);

        // Reachable: bob (hop1), carol, dave (hop2) = 3 recipients, 300 each
        let token = TokenClient::new(&env, &token_id);
        assert_eq!(token.balance(&bob), 300);
        assert_eq!(token.balance(&carol), 300);
        assert_eq!(token.balance(&dave), 300);
    }

    #[test]
    #[should_panic(expected = "insufficient deposit")]
    fn test_overdraw_panics() {
        let (env, contract_id, token_id) = setup();
        let alice = Address::generate(&env);
        let bob = Address::generate(&env);
        mint(&env, &token_id, &alice, 100);

        let client = TrustFlowClient::new(&env, &contract_id);
        client.set_trust(&alice, &vec![&env, bob.clone()]);
        client.deposit(&alice, &token_id, &100);
        client.distribute(&alice, &token_id, &200, &1); // more than deposited
    }

    #[test]
    #[should_panic(expected = "no reachable peers")]
    fn test_no_peers_panics() {
        let (env, contract_id, token_id) = setup();
        let alice = Address::generate(&env);
        mint(&env, &token_id, &alice, 100);

        let client = TrustFlowClient::new(&env, &contract_id);
        // Alice has no trust slice set
        client.deposit(&alice, &token_id, &100);
        client.distribute(&alice, &token_id, &100, &1);
    }
}
