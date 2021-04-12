use near_sdk::collections::LookupMap;

use near_contract_standards::fungible_token::{
    core::FungibleTokenCore,
    metadata::{FungibleTokenMetadata, FungibleTokenMetadataProvider, FT_METADATA_SPEC},
    resolver::FungibleTokenResolver,
};

use near_sdk::borsh::{self, BorshDeserialize, BorshSerialize};
use near_sdk::collections::LazyOption;
use near_sdk::json_types::{ValidAccountId, U128};
use near_sdk::{
    assert_one_yocto, env, ext_contract, log, near_bindgen, AccountId, Balance, Gas,
    PanicOnDefault, PromiseOrValue, StorageUsage,
};

const TGAS: Gas = 1_000_000_000_000;
const GAS_FOR_RESOLVE_TRANSFER: Gas = 5*TGAS;
const GAS_FOR_FT_TRANSFER_CALL: Gas = 25*TGAS + GAS_FOR_RESOLVE_TRANSFER;
const NO_DEPOSIT: Balance = 0;

type U128String = U128;

near_sdk::setup_alloc!();

mod internal;
use internal::*;

#[near_bindgen]
#[derive(BorshDeserialize, BorshSerialize, PanicOnDefault)]
pub struct Contract {
    metadata: LazyOption<FungibleTokenMetadata>,

    pub accounts: LookupMap<AccountId, Balance>,

    pub owner_id: AccountId,
    pub total_supply: Balance,
    
    /// The storage size in bytes for one account.
    pub account_storage_usage: StorageUsage,
}

#[near_bindgen]
impl Contract {
    /// Initializes the contract with the given total supply owned by the given `owner_id`.

    #[init]
    pub fn new(owner_id: ValidAccountId, owner_supply: U128) -> Self {
        let m = FungibleTokenMetadata {
            spec: FT_METADATA_SPEC.to_string(),
            name: "Chedder".to_string(),
            symbol: "CHDR".to_string(),
            icon: Some(String::from(r###"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 56 56"><style>.a{fill:#F4C647;}.b{fill:#EEAF4B;}</style><path d="M45 19.5v5.5l4.8 0.6 0-11.4c-0.1-3.2-11.2-6.7-24.9-6.7 -13.7 0-24.8 3.6-24.9 6.7L0 32.5c0 3.2 10.7 7.1 24.5 7.1 0.2 0 0.3 0 0.5 0V21.5l-4.7-7.2L45 19.5z" class="a"/><path d="M25 31.5v-10l-4.7-7.2L45 19.5v5.5l-14-1.5v10C31 33.5 25 31.5 25 31.5z" fill="#F9E295"/><path d="M24.9 7.5C11.1 7.5 0 11.1 0 14.3s10.7 7.2 24.5 7.2c0.2 0 0.3 0 0.5 0l-4.7-7.2 25 5.2c2.8-0.9 4.4-4 4.4-5.2C49.8 11.1 38.6 7.5 24.9 7.5z" class="b"/><path d="M36 29v19.6c8.3 0 15.6-1 20-2.5V26.5L31 23.2 36 29z" class="a"/><path d="M31 23.2l5 5.8c8.2 0 15.6-1 19.9-2.5L31 23.2z" class="b"/><polygon points="36 29 36 48.5 31 42.5 31 23.2 " fill="#FCDF76"/></svg>"###)),
            reference: None, // TODO
            reference_hash: None,
            decimals: 24,
        };
        m.assert_valid();
        let mut this = Self {
            owner_id: owner_id.clone().into(),
            metadata: LazyOption::new(b"m".to_vec(), Some(&m)),
            accounts: LookupMap::new(b"a".to_vec()),
            total_supply: 0,
            account_storage_usage: 0,
        };
        this.account_storage_usage = measure_account_storage(&mut this.accounts);
        this.internal_deposit(owner_id.as_ref(), owner_supply.into());
        this
    }

    //owner can mint more
    pub fn mint(&mut self, amount:U128String){
        &self.assert_owner_calling();
        self.total_supply = self.total_supply.checked_add(amount.0).unwrap();
        self.internal_deposit(&self.owner_id.clone(), amount.0);
    }

    /// Returns account ID of the staking pool owner.
    pub fn get_owner_id(&self) -> AccountId {
        return self.owner_id.clone();
    }

    /// Returns account ID of the staking pool owner.
    #[payable]
    pub fn set_metadata_icon(&mut self, svg_string: String)  {
        assert_one_yocto();
        self.assert_owner_calling();
        let mut m = self.metadata.get().unwrap();
        m.icon = Some(svg_string);
        self.metadata.set(&m);
    }

    /// Returns account ID of the staking pool owner.
    #[payable]
    pub fn set_metadata_reference(&mut self, reference: String, reference_hash:String)  {
        assert_one_yocto();
        self.assert_owner_calling();
        let mut m = self.metadata.get().unwrap();
        m.reference = Some(reference);
        m.reference_hash = Some(reference_hash.as_bytes().to_vec().into());
        m.assert_valid();
        self.metadata.set(&m);
    }

}

#[near_bindgen]
impl FungibleTokenCore for Contract {
    #[payable]
    fn ft_transfer(&mut self, receiver_id: ValidAccountId, amount: U128, memo: Option<String>) {
        assert_one_yocto();
        let sender_id = env::predecessor_account_id();
        let amount: Balance = amount.into();
        self.internal_transfer(&sender_id, receiver_id.as_ref(), amount, memo);
    }

    #[payable]
    fn ft_transfer_call(
        &mut self,
        receiver_id: ValidAccountId,
        amount: U128,
        memo: Option<String>,
        msg: String,
    ) -> PromiseOrValue<U128> {
        assert_one_yocto();
        let sender_id = env::predecessor_account_id();
        let amount: Balance = amount.into();
        self.internal_transfer(&sender_id, receiver_id.as_ref(), amount, memo);
        // Initiating receiver's call and the callback
        // ext_fungible_token_receiver::ft_on_transfer(
        ext_ft_receiver::ft_on_transfer(
            sender_id.clone(),
            amount.into(),
            msg,
            receiver_id.as_ref(),
            NO_DEPOSIT,
            env::prepaid_gas() - GAS_FOR_FT_TRANSFER_CALL,
        )
        .then(ext_self::ft_resolve_transfer(
            sender_id,
            receiver_id.into(),
            amount.into(),
            &env::current_account_id(),
            NO_DEPOSIT,
            GAS_FOR_RESOLVE_TRANSFER,
        ))
        .into()
    }

    fn ft_total_supply(&self) -> U128 {
        self.total_supply.into()
    }

    fn ft_balance_of(&self, account_id: ValidAccountId) -> U128 {
        self.accounts.get(account_id.as_ref()).unwrap_or(0).into()
    }
}

#[near_bindgen]
impl FungibleTokenResolver for Contract {
    /// Returns the amount of burned tokens in a corner case when the sender
    /// has deleted (unregistered) their account while the `ft_transfer_call` was still in flight.
    /// Returns (Used token amount, Burned token amount)
    #[private]
    fn ft_resolve_transfer(
        &mut self,
        sender_id: ValidAccountId,
        receiver_id: ValidAccountId,
        amount: U128,
    ) -> U128 {
        let sender_id: AccountId = sender_id.into();
        let (used_amount, burned_amount) =
            self.int_ft_resolve_transfer(&sender_id, receiver_id, amount);
        if burned_amount > 0 {
            log!("{} tokens burned", burned_amount);
        }
        return used_amount.into();
    }
}

#[near_bindgen]
impl FungibleTokenMetadataProvider for Contract {
    fn ft_metadata(&self) -> FungibleTokenMetadata {
        self.metadata.get().unwrap()
    }
}


#[ext_contract(ext_ft_receiver)]
pub trait FungibleTokenReceiver {
    fn ft_on_transfer(
        &mut self,
        sender_id: AccountId,
        amount: U128,
        msg: String,
    ) -> PromiseOrValue<U128>;
}

#[ext_contract(ext_self)]
trait FungibleTokenResolver {
    fn ft_resolve_transfer(
        &mut self,
        sender_id: AccountId,
        receiver_id: AccountId,
        amount: U128,
    ) -> U128;
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use near_sdk::test_utils::{accounts, VMContextBuilder};
    use near_sdk::MockedBlockchain;
    use near_sdk::{testing_env, Balance};

    use super::*;

    const OWNER_SUPPLY: Balance = 1_000_000_000_000_000;

    fn get_context(predecessor_account_id: ValidAccountId) -> VMContextBuilder {
        let mut builder = VMContextBuilder::new();
        builder
            .current_account_id(accounts(0))
            .signer_account_id(predecessor_account_id.clone())
            .predecessor_account_id(predecessor_account_id);
        builder
    }

    #[test]
    fn test_new() {
        let mut context = get_context(accounts(1));
        testing_env!(context.build());
        let contract = Contract::new(accounts(1).into(), OWNER_SUPPLY.into());
        testing_env!(context.is_view(true).build());
        assert_eq!(contract.ft_total_supply().0, OWNER_SUPPLY);
        assert_eq!(contract.ft_balance_of(accounts(1)).0, OWNER_SUPPLY);
    }

    #[test]
    #[should_panic(expected = "The contract is not initialized")]
    fn test_default() {
        let context = get_context(accounts(1));
        testing_env!(context.build());
        let _contract = Contract::default();
    }

    #[test]
    fn test_transfer() {
        let mut context = get_context(accounts(2));
        testing_env!(context.build());
        let mut contract = Contract::new(accounts(2).into(), OWNER_SUPPLY.into());
        testing_env!(context
            .storage_usage(env::storage_usage())
            .attached_deposit(contract.storage_balance_bounds().min.into())
            .predecessor_account_id(accounts(1))
            .build());
        // Paying for account registration, aka storage deposit
        contract.storage_deposit(None, None);

        testing_env!(context
            .storage_usage(env::storage_usage())
            .attached_deposit(1)
            .predecessor_account_id(accounts(2))
            .build());
        let transfer_amount = OWNER_SUPPLY / 3;
        contract.ft_transfer(accounts(1), transfer_amount.into(), None);

        testing_env!(context
            .storage_usage(env::storage_usage())
            .account_balance(env::account_balance())
            .is_view(true)
            .attached_deposit(0)
            .build());
        assert_eq!(
            contract.ft_balance_of(accounts(2)).0,
            (OWNER_SUPPLY - transfer_amount)
        );
        assert_eq!(contract.ft_balance_of(accounts(1)).0, transfer_amount);
    }
}
