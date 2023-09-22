use crate::balance::{Amount, ArithmeticError, Balance};
use crate::client::ExecutionError::{InvalidBooking, InvalidState};
use crate::client::State::{Booked, Chargeback, Disputed, Resolved};
use std::collections::HashMap;

pub type ClientId = u16;
pub type TransactionId = u32;

type BookingMap = HashMap<TransactionId, BookedDeposit>;

#[derive(Clone)]
pub struct Client {
    id: ClientId,
    balance: Balance,
    bookings: BookingMap,
    locked: bool,
}

impl Client {
    pub fn new(id: ClientId) -> Self {
        Self {
            id,
            locked: false,
            balance: Balance::default(),
            bookings: BookingMap::default(),
        }
    }

    pub fn id(&self) -> ClientId {
        self.id
    }

    pub fn balance(&self) -> &Balance {
        &self.balance
    }

    pub fn lock(&mut self) {
        self.locked = true;
    }

    pub fn is_locked(&self) -> bool {
        self.locked
    }

    pub fn get_booking_mut(&mut self, tx_id: &TransactionId) -> Result<&mut BookedDeposit, ExecutionError> {
        self.bookings.get_mut(tx_id).ok_or(InvalidBooking)
    }

    pub fn add_or_update_booking(&mut self, deposit: BookedDeposit) {
        self.bookings.insert(deposit.tx, deposit);
    }

    pub fn get_balance_mut(&mut self) -> &mut Balance {
        &mut self.balance
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum State {
    Booked,
    Disputed,
    Resolved,
    Chargeback,
}

#[derive(Clone)]
pub struct BookedDeposit {
    tx: TransactionId,
    amount: Amount,
    state: State,
}

impl BookedDeposit {
    pub fn new(tx: TransactionId, amount: Amount) -> Self {
        Self {
            state: Booked,
            tx,
            amount,
        }
    }

    pub fn dispute(&mut self) -> Result<(), ExecutionError> {
        self.try_change_state(Booked, Disputed)
    }

    pub fn resolve(&mut self) -> Result<(), ExecutionError> {
        self.try_change_state(Disputed, Resolved)
    }

    pub fn chargeback(&mut self) -> Result<(), ExecutionError> {
        self.try_change_state(Disputed, Chargeback)
    }

    pub fn amount(&self) -> Amount {
        self.amount
    }

    pub fn state(&self) -> &State {
        &self.state
    }

    fn try_change_state(&mut self, from: State, to: State) -> Result<(), ExecutionError> {
        if self.state == from {
            self.state = to;
            Ok(())
        } else {
            Err(InvalidState)
        }
    }
}

#[derive(Debug, PartialEq)]
pub enum ExecutionError {
    InvalidState,
    InvalidBooking,
    ClientLocked,
    ClientDoesNotExist,
    Arithmetic(ArithmeticError),
}

#[cfg(test)]
mod tests {
    use crate::client::State::*;
    use crate::client::{BookedDeposit, State};

    #[test]
    fn dispute() {
        let mut deposit = deposit_with_state(Booked);

        assert!(deposit.resolve().is_err());
        assert_eq!(deposit.state, Booked);
        assert!(deposit.chargeback().is_err());
        assert_eq!(deposit.state, Booked);
        assert!(deposit.dispute().is_ok());
        assert_eq!(deposit.state, Disputed);
    }

    #[test]
    fn resolve() {
        let mut deposit = deposit_with_state(Disputed);

        assert!(deposit.dispute().is_err());
        assert_eq!(deposit.state, Disputed);
        assert!(deposit.resolve().is_ok());
        assert_eq!(deposit.state, Resolved);
    }

    #[test]
    fn chargeback() {
        let mut deposit = deposit_with_state(Disputed);

        assert!(deposit.dispute().is_err());
        assert_eq!(deposit.state, Disputed);
        assert!(deposit.chargeback().is_ok());
        assert_eq!(deposit.state, Chargeback);
    }

    fn deposit_with_state(state: State) -> BookedDeposit {
        BookedDeposit {
            state,
            amount: 0,
            tx: 0,
        }
    }
}
