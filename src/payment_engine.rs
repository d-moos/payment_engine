use crate::balance::{Amount, ArithmeticError};
use crate::client::ExecutionError::{Arithmetic, ClientDoesNotExist, ClientLocked};
use crate::client::{BookedDeposit, Client, ClientId, ExecutionError, TransactionId};
use std::collections::hash_map::Entry;
use std::collections::HashMap;

pub type ClientMap = HashMap<ClientId, Client>;

impl From<ArithmeticError> for ExecutionError {
    fn from(value: ArithmeticError) -> Self {
        Arithmetic(value)
    }
}

#[derive(Default)]
pub struct PaymentEngine {
    clients: ClientMap,
}

impl PaymentEngine {
    /// Executes a given Transaction and updates the client state
    ///
    /// executes a transaction and - if successful - updates the internal client state
    /// if any error occurs during execution the client is not updated.
    pub fn execute(&mut self, transaction: Transaction) -> Result<(), ExecutionError> {
        // try retrieve a previously stored client
        let mut client = match self.clients.entry(transaction.client) {
            // create a copy of it so that we do not mutate the state immediately
            Entry::Occupied(e) => Ok(e.get().clone()),
            // if the client does not exist
            Entry::Vacant(_) => match transaction.transaction_type {
                // ...and the transaction is a deposit, create a new one
                TransactionType::Deposit(_) => Ok(Client::new(transaction.client)),
                // ... or return an error for all other tx types
                _ => Err(ClientDoesNotExist),
            },
        }?;

        // do not proceed if the client has been previously locked
        if client.is_locked() {
            return Err(ClientLocked);
        }

        match transaction.transaction_type {
            TransactionType::Deposit(amount) => self.deposit(&mut client, amount, transaction.id),
            TransactionType::Withdrawal(amount) => self.withdraw(&mut client, amount),
            TransactionType::Dispute | TransactionType::Resolve | TransactionType::Chargeback => {
                // try and get previously booked deposit
                let mut booking = client.get_booking_mut(&transaction.id)?.clone();

                match transaction.transaction_type {
                    TransactionType::Dispute => {
                        // check if disputable
                        booking.dispute()?;

                        // freeze amount
                        client.get_balance_mut().freeze(booking.amount())?;
                    }
                    TransactionType::Resolve => {
                        // check if resolvable
                        booking.resolve()?;

                        // unfreeze amount
                        client.get_balance_mut().unfreeze(booking.amount())?;
                    }
                    TransactionType::Chargeback => {
                        // check if chargeback is possible
                        booking.chargeback()?;

                        // chargeback amount
                        client.get_balance_mut().chargeback(booking.amount())?;

                        // clients are locked if they chargeback
                        client.lock();
                    }
                    _ => unreachable!(
                        "this path is only reachable through Dispute, Resolve or Chargeback"
                    ),
                }

                // update booking with cloned value
                client.add_or_update_booking(booking);

                Ok(())
            }
        }?;

        // update client
        self.clients.insert(transaction.client, client);

        Ok(())
    }

    /// consumes the engine into client vec
    ///
    /// exposes all clients as a vector, so that we can finalize the payment process
    pub fn into_clients(self) -> Vec<Client> {
        self.clients.into_values().map(|kv| kv).collect()
    }

    fn deposit(
        &mut self,
        client: &mut Client,
        amount: Amount,
        tx: TransactionId,
    ) -> Result<(), ExecutionError> {
        // update balance
        client.get_balance_mut().credit(amount)?;

        // add booking
        client.add_or_update_booking(BookedDeposit::new(tx, amount));

        Ok(())
    }

    fn withdraw(&mut self, client: &mut Client, amount: Amount) -> Result<(), ExecutionError> {
        // update balance
        client.get_balance_mut().debit(amount)?;

        Ok(())
    }
}

pub struct Transaction {
    id: TransactionId,
    pub client: ClientId,
    transaction_type: TransactionType,
}

impl Transaction {
    pub fn new(id: TransactionId, client: ClientId, transaction_type: TransactionType) -> Self {
        Self {
            client,
            transaction_type,
            id,
        }
    }
}

pub enum TransactionType {
    Deposit(Amount),
    Withdrawal(Amount),
    Dispute,
    Resolve,
    Chargeback,
}

#[cfg(test)]
mod tests {
    use crate::balance::Balance;
    use crate::client::ExecutionError::ClientLocked;
    use crate::client::{Client, ClientId};
    use crate::payment_engine::TransactionType::Deposit;
    use crate::payment_engine::{ClientMap, PaymentEngine, Transaction};

    #[test]
    fn cannot_operate_on_locked_account() {
        const CLIENT: ClientId = 1;

        // create initial balance for the client
        let mut engine = engine_with_client(CLIENT, Balance::default());
        engine.clients.get_mut(&CLIENT).unwrap().lock();

        assert_eq!(
            engine
                .execute(Transaction {
                    id: 1,
                    client: CLIENT,
                    transaction_type: Deposit(100)
                })
                .unwrap_err(),
            ClientLocked
        );
    }

    #[cfg(test)]
    mod deposit {
        use crate::balance::ArithmeticError::Overflow;
        use crate::balance::{Amount, Balance};
        use crate::client::ClientId;
        use crate::client::ExecutionError::Arithmetic;
        use crate::payment_engine::tests::engine_with_client;
        use crate::payment_engine::TransactionType::Deposit;
        use crate::payment_engine::{PaymentEngine, Transaction};

        #[test]
        fn deposit_creates_client() {
            const CLIENT: ClientId = 1;

            let mut engine = PaymentEngine::default();
            assert!(engine
                .execute(Transaction {
                    transaction_type: Deposit(100),
                    client: CLIENT,
                    id: 1,
                })
                .is_ok());

            assert!(engine.clients.contains_key(&CLIENT));
        }

        #[test]
        fn successful_deposit_updates_balance() {
            const CLIENT: ClientId = 1;
            const DEPOSIT: Amount = 50;

            let mut engine = PaymentEngine::default();
            assert!(engine
                .execute(Transaction {
                    transaction_type: Deposit(50),
                    client: CLIENT,
                    id: 1,
                })
                .is_ok());

            let client = engine.clients.get(&CLIENT).unwrap();
            assert_eq!(client.balance().available(), DEPOSIT);
        }

        #[test]
        fn overflowing_deposit_does_not_modify_client() {
            const CLIENT: ClientId = 1;

            let mut init_balance = Balance::default();
            init_balance.credit(Amount::MAX).unwrap();

            let mut engine = engine_with_client(CLIENT, init_balance);
            assert_eq!(
                engine.execute(Transaction {
                    transaction_type: Deposit(50),
                    client: CLIENT,
                    id: 1,
                }),
                Err(Arithmetic(Overflow))
            );

            let client = engine.clients.get(&CLIENT).unwrap();
            assert_eq!(client.balance().available(), Amount::MAX);
        }
    }

    #[cfg(test)]
    mod withdrawal {
        use crate::balance::ArithmeticError::Underflow;
        use crate::balance::{Amount, Balance};
        use crate::client::ClientId;
        use crate::client::ExecutionError::{Arithmetic, ClientDoesNotExist};
        use crate::payment_engine::tests::engine_with_client;
        use crate::payment_engine::TransactionType::Withdrawal;
        use crate::payment_engine::{PaymentEngine, Transaction};

        #[test]
        fn cannot_withdraw_if_client_does_not_exist() {
            const CLIENT: ClientId = 1;

            let mut engine = PaymentEngine::default();
            assert_eq!(
                engine.execute(Transaction {
                    transaction_type: Withdrawal(100),
                    client: CLIENT,
                    id: 1,
                }),
                Err(ClientDoesNotExist)
            );
        }

        #[test]
        fn successful_withdrawal_updates_balance() {
            const CLIENT: ClientId = 1;
            const BALANCE: Amount = 100;
            const WITHDRAW: Amount = 70;

            let mut init_balance = Balance::default();
            init_balance.credit(BALANCE).unwrap();

            let mut engine = engine_with_client(CLIENT, init_balance);

            assert!(engine
                .execute(Transaction {
                    id: 1,
                    client: CLIENT,
                    transaction_type: Withdrawal(WITHDRAW),
                })
                .is_ok());

            let client = engine.clients.get(&CLIENT).unwrap();
            assert_eq!(client.balance().available(), BALANCE - WITHDRAW);
        }

        #[test]
        fn underflowing_withdrawal_does_not_update_client() {
            const CLIENT: ClientId = 1;
            const BALANCE: Amount = 100;
            const WITHDRAW: Amount = 120;

            let mut init_balance = Balance::default();
            init_balance.credit(BALANCE).unwrap();

            let mut engine = engine_with_client(CLIENT, init_balance);

            assert_eq!(
                engine.execute(Transaction {
                    id: 1,
                    client: CLIENT,
                    transaction_type: Withdrawal(WITHDRAW),
                }),
                Err(Arithmetic(Underflow))
            );

            let client = engine.clients.get(&CLIENT).unwrap();
            assert_eq!(client.balance().available(), BALANCE);
        }
    }

    #[cfg(test)]
    mod dispute {
        use crate::balance::{Amount, Balance};
        use crate::client::ExecutionError::{ClientDoesNotExist, InvalidState};
        use crate::client::{BookedDeposit, ClientId, State, TransactionId};
        use crate::payment_engine::tests::engine_with_client;
        use crate::payment_engine::TransactionType::{Deposit, Dispute};
        use crate::payment_engine::{PaymentEngine, Transaction};

        #[test]
        fn cannot_dispute_if_client_does_not_exist() {
            const CLIENT: ClientId = 1;

            let mut engine = PaymentEngine::default();
            assert_eq!(
                engine.execute(Transaction {
                    transaction_type: Dispute,
                    client: CLIENT,
                    id: 1,
                }),
                Err(ClientDoesNotExist)
            );
        }

        #[test]
        fn successful_dispute_updates_balance_and_transaction() {
            const CLIENT: ClientId = 1;
            const DEPOSIT: Amount = 100;
            const TRANSACTION: TransactionId = 2;

            let mut engine = PaymentEngine::default();
            assert!(engine
                .execute(Transaction {
                    transaction_type: Deposit(DEPOSIT),
                    client: CLIENT,
                    id: TRANSACTION,
                })
                .is_ok());

            assert!(engine
                .execute(Transaction {
                    id: TRANSACTION,
                    client: CLIENT,
                    transaction_type: Dispute,
                })
                .is_ok());

            let client = engine.clients.get_mut(&CLIENT).unwrap();
            assert_eq!(client.balance().available(), 0);
            assert_eq!(client.balance().frozen(), DEPOSIT);

            let booking = client.get_booking_mut(&TRANSACTION).unwrap();
            assert_eq!(*booking.state(), State::Disputed);
        }

        #[test]
        fn invalid_dispute_does_not_update_balance_and_transaction() {
            const CLIENT: ClientId = 1;
            const DEPOSIT: Amount = 100;
            const TRANSACTION: TransactionId = 2;

            // create initial balance for the client
            let mut init_balance = Balance::default();
            init_balance.credit(DEPOSIT).unwrap();
            let mut engine = engine_with_client(CLIENT, init_balance);

            // create a booking that is in state `Resolved`
            let mut booking = BookedDeposit::new(TRANSACTION, DEPOSIT);
            assert!(booking.dispute().is_ok());
            assert!(booking.resolve().is_ok());
            engine
                .clients
                .get_mut(&CLIENT)
                .unwrap()
                .add_or_update_booking(booking);

            assert_eq!(
                engine
                    .execute(Transaction {
                        id: TRANSACTION,
                        client: CLIENT,
                        transaction_type: Dispute,
                    })
                    .unwrap_err(),
                InvalidState
            );

            let client = engine.clients.get_mut(&CLIENT).unwrap();
            assert_eq!(client.balance().available(), DEPOSIT);
            assert_eq!(client.balance().frozen(), 0);

            let booking = client.get_booking_mut(&TRANSACTION).unwrap();
            assert_eq!(*booking.state(), State::Resolved);
        }
    }

    #[cfg(test)]
    mod resolve {
        use crate::balance::{Amount, Balance};
        use crate::client::ExecutionError::{ClientDoesNotExist, InvalidState};
        use crate::client::{BookedDeposit, ClientId, State, TransactionId};
        use crate::payment_engine::tests::engine_with_client;
        use crate::payment_engine::TransactionType::Resolve;
        use crate::payment_engine::{PaymentEngine, Transaction};

        #[test]
        fn cannot_resolve_if_client_does_not_exist() {
            const CLIENT: ClientId = 1;

            let mut engine = PaymentEngine::default();
            assert_eq!(
                engine.execute(Transaction {
                    transaction_type: Resolve,
                    client: CLIENT,
                    id: 1,
                }),
                Err(ClientDoesNotExist)
            );
        }

        #[test]
        fn successful_resolve_updates_balance_and_transaction() {
            const CLIENT: ClientId = 1;
            const DEPOSIT: Amount = 100;
            const TRANSACTION: TransactionId = 2;

            // create initial balance for the client
            let mut init_balance = Balance::default();
            init_balance.credit(DEPOSIT).unwrap();
            init_balance.freeze(DEPOSIT).unwrap();
            let mut engine = engine_with_client(CLIENT, init_balance);

            // create a booking that is in state `Disputed`
            let mut booking = BookedDeposit::new(TRANSACTION, DEPOSIT);
            assert!(booking.dispute().is_ok());
            engine
                .clients
                .get_mut(&CLIENT)
                .unwrap()
                .add_or_update_booking(booking);

            assert!(engine
                .execute(Transaction {
                    id: TRANSACTION,
                    client: CLIENT,
                    transaction_type: Resolve,
                })
                .is_ok());

            let client = engine.clients.get_mut(&CLIENT).unwrap();
            assert_eq!(client.balance().available(), DEPOSIT);
            assert_eq!(client.balance().frozen(), 0);

            let booking = client.get_booking_mut(&TRANSACTION).unwrap();
            assert_eq!(*booking.state(), State::Resolved);
        }

        #[test]
        fn invalid_resolve_does_not_update_balance_and_transaction() {
            const CLIENT: ClientId = 1;
            const DEPOSIT: Amount = 100;
            const TRANSACTION: TransactionId = 2;

            // create initial balance for the client
            let mut init_balance = Balance::default();
            init_balance.credit(DEPOSIT).unwrap();
            let mut engine = engine_with_client(CLIENT, init_balance);

            // create a booking that is in state `Booked`
            let booking = BookedDeposit::new(TRANSACTION, DEPOSIT);
            engine
                .clients
                .get_mut(&CLIENT)
                .unwrap()
                .add_or_update_booking(booking);

            assert_eq!(
                engine
                    .execute(Transaction {
                        id: TRANSACTION,
                        client: CLIENT,
                        transaction_type: Resolve,
                    })
                    .unwrap_err(),
                InvalidState
            );

            let client = engine.clients.get_mut(&CLIENT).unwrap();
            assert_eq!(client.balance().available(), DEPOSIT);
            assert_eq!(client.balance().frozen(), 0);

            let booking = client.get_booking_mut(&TRANSACTION).unwrap();
            assert_eq!(*booking.state(), State::Booked);
        }
    }

    #[cfg(test)]
    mod chargeback {
        use crate::balance::{Amount, Balance};
        use crate::client::ExecutionError::{ClientDoesNotExist, InvalidState};
        use crate::client::{BookedDeposit, ClientId, State, TransactionId};
        use crate::payment_engine::tests::engine_with_client;
        use crate::payment_engine::TransactionType::Chargeback;
        use crate::payment_engine::{PaymentEngine, Transaction};

        #[test]
        fn cannot_chargeback_if_client_does_not_exist() {
            const CLIENT: ClientId = 1;

            let mut engine = PaymentEngine::default();
            assert_eq!(
                engine.execute(Transaction {
                    transaction_type: Chargeback,
                    client: CLIENT,
                    id: 1,
                }),
                Err(ClientDoesNotExist)
            );
        }

        #[test]
        fn successful_chargeback_updates_client_and_transaction() {
            const CLIENT: ClientId = 1;
            const DEPOSIT: Amount = 100;
            const TRANSACTION: TransactionId = 2;

            // create initial balance for the client
            let mut init_balance = Balance::default();
            init_balance.credit(DEPOSIT).unwrap();
            init_balance.freeze(DEPOSIT).unwrap();
            let mut engine = engine_with_client(CLIENT, init_balance);

            // create a booking that is in state `Disputed`
            let mut booking = BookedDeposit::new(TRANSACTION, DEPOSIT);
            assert!(booking.dispute().is_ok());
            engine
                .clients
                .get_mut(&CLIENT)
                .unwrap()
                .add_or_update_booking(booking);

            assert!(engine
                .execute(Transaction {
                    id: TRANSACTION,
                    client: CLIENT,
                    transaction_type: Chargeback,
                })
                .is_ok());

            let client = engine.clients.get_mut(&CLIENT).unwrap();
            assert_eq!(client.balance().available(), 0);
            assert_eq!(client.balance().frozen(), 0);

            let booking = client.get_booking_mut(&TRANSACTION).unwrap();
            assert_eq!(*booking.state(), State::Chargeback);

            assert!(client.is_locked());
        }

        #[test]
        fn invalid_chargeback_does_not_update_balance_and_transaction() {
            const CLIENT: ClientId = 1;
            const DEPOSIT: Amount = 100;
            const TRANSACTION: TransactionId = 2;

            // create initial balance for the client
            let mut init_balance = Balance::default();
            init_balance.credit(DEPOSIT).unwrap();
            let mut engine = engine_with_client(CLIENT, init_balance);

            // create a booking that is in state `Booked`
            let booking = BookedDeposit::new(TRANSACTION, DEPOSIT);
            engine
                .clients
                .get_mut(&CLIENT)
                .unwrap()
                .add_or_update_booking(booking);

            assert_eq!(
                engine
                    .execute(Transaction {
                        id: TRANSACTION,
                        client: CLIENT,
                        transaction_type: Chargeback,
                    })
                    .unwrap_err(),
                InvalidState
            );

            let client = engine.clients.get_mut(&CLIENT).unwrap();
            assert_eq!(client.balance().available(), DEPOSIT);
            assert_eq!(client.balance().frozen(), 0);

            let booking = client.get_booking_mut(&TRANSACTION).unwrap();
            assert_eq!(*booking.state(), State::Booked);

            assert!(!client.is_locked());
        }
    }

    fn engine_with_client(id: ClientId, balance: Balance) -> PaymentEngine {
        let mut clients = ClientMap::default();
        let mut client = Client::new(id);
        *client.get_balance_mut() = balance;

        clients.insert(id, client);

        PaymentEngine { clients }
    }
}
