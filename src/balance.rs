use crate::balance::ArithmeticError::{Overflow, Underflow};

pub type Amount = u64;
type BalanceResult = Result<(), ArithmeticError>;

#[derive(Default, Clone, Debug)]
pub struct Balance {
    frozen: Amount,
    available: Amount,
}

#[derive(PartialEq, Debug)]
pub enum ArithmeticError {
    Overflow,
    Underflow,
}

impl Balance {
    pub fn frozen(&self) -> Amount {
        self.frozen
    }

    pub fn available(&self) -> Amount {
        self.available
    }

    /// freezes a given amount of an account balance
    ///
    /// moves a specified amount from `available` to `frozen`.
    ///
    /// # Examples
    /// ```
    /// let mut account = Balance::default();
    /// account.deposit(100);
    /// account.freeze(50);
    /// assert_eq!(account.frozen, 50);
    /// assert_eq!(account.available, 50);
    /// ```
    /// # Errors
    /// - [Overflow] if `frozen` exceeds the max value
    /// - [Underflow] if `available` falls below the min value
    pub fn freeze(&mut self, amount: Amount) -> BalanceResult {
        let available = self.available.checked_sub(amount).ok_or(Underflow)?;
        let frozen = self.frozen.checked_add(amount).ok_or(Overflow)?;

        self.available = available;
        self.frozen = frozen;

        Ok(())
    }

    /// unfreezes a given amount of an account balance
    ///
    /// moves a specified amount from `frozen` to `available`.
    ///
    /// # Examples
    /// ```
    /// let mut account = Balance::default();
    /// account.deposit(100);
    /// account.freeze(50);
    /// account.unfreeze(10);
    /// assert_eq!(account.frozen, 40);
    /// assert_eq!(account.available, 60);
    /// ```
    /// # Errors
    /// - [Overflow] if `available` exceeds the max value
    /// - [Underflow] if `frozen` falls below the min value
    pub fn unfreeze(&mut self, amount: Amount) -> BalanceResult {
        let available = self.available.checked_add(amount).ok_or(Overflow)?;
        let frozen = self.frozen.checked_sub(amount).ok_or(Underflow)?;

        self.available = available;
        self.frozen = frozen;

        Ok(())
    }

    /// adds a given amount to the account balance
    ///
    /// adds a specified amount to `available`.
    ///
    /// # Examples
    /// ```
    /// let mut account = Balance::default();
    /// account.deposit(100);
    /// assert_eq!(account.available, 100);
    /// ```
    /// # Errors
    /// - [Overflow] if `available` exceeds the max value
    pub fn credit(&mut self, amount: Amount) -> BalanceResult {
        let available = self.available.checked_add(amount).ok_or(Overflow)?;

        // ensure that available + frozen (total) does not overflow
        available.checked_add(self.frozen).ok_or(Overflow)?;

        self.available = available;

        Ok(())
    }

    /// withdraws a given amount from the account balance
    ///
    /// subtracts a specified amount from `available`.
    ///
    /// # Examples
    /// ```
    /// let mut account = Balance::default();
    /// account.deposit(100);
    /// account.withdraw(10);
    /// assert_eq!(account.available, 90);
    /// ```
    /// # Errors
    /// - [Underflow] if `available` falls below the min value
    pub fn debit(&mut self, amount: Amount) -> BalanceResult {
        self.available = self.available.checked_sub(amount).ok_or(Underflow)?;

        Ok(())
    }

    /// removes a given amount from the account balance
    ///
    /// subtracts a specified amount from `frozen`.
    ///
    /// # Examples
    /// ```
    /// let mut account = Balance::default();
    /// account.deposit(100);
    /// account.freeze(20);
    /// assert_eq!(account.available, 80);
    /// assert_eq!(account.frozen, 20);
    /// account.chargeback(20);
    /// assert_eq!(account.available, 80);
    /// assert_eq!(account.frozen, 0);
    /// ```
    /// # Errors
    /// - [Underflow] if `frozen` falls below the min value
    pub fn chargeback(&mut self, amount: Amount) -> BalanceResult {
        self.frozen = self.frozen.checked_sub(amount).ok_or(Underflow)?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::balance::ArithmeticError::{Overflow, Underflow};
    use crate::balance::{Amount, Balance};

    #[test]
    fn deposit_works() {
        const DEPOSIT_AMOUNT: Amount = 500;

        let mut balance = Balance::default();
        assert!(balance.credit(DEPOSIT_AMOUNT).is_ok());
        assert_eq!(balance.available, DEPOSIT_AMOUNT);
        assert_eq!(balance.frozen, 0);
    }

    #[test]
    fn deposit_overflow_check() {
        const INITIAL_DEPOSIT_AMOUNT: Amount = Amount::MAX;

        let mut balance = Balance::default();
        assert!(balance.credit(INITIAL_DEPOSIT_AMOUNT).is_ok());
        assert_eq!(balance.credit(1).unwrap_err(), Overflow);
    }

    #[test]
    fn deposit_cannot_overflow_total_balance() {
        const DEPOSIT_AMOUNT: Amount = Amount::MAX;

        let mut balance = Balance::default();
        assert!(balance.credit(DEPOSIT_AMOUNT).is_ok());
        assert!(balance.freeze(DEPOSIT_AMOUNT).is_ok());
        assert_eq!(balance.credit(DEPOSIT_AMOUNT).unwrap_err(), Overflow);
    }

    #[test]
    fn withdraw_works() {
        const DEPOSIT_AMOUNT: Amount = 500;
        const WITHDRAW_AMOUNT: Amount = DEPOSIT_AMOUNT - 50;

        let mut balance = Balance::default();
        assert!(balance.credit(DEPOSIT_AMOUNT).is_ok());
        assert!(balance.debit(WITHDRAW_AMOUNT).is_ok());
        assert_eq!(balance.available, DEPOSIT_AMOUNT - WITHDRAW_AMOUNT);
        assert_eq!(balance.frozen, 0);
    }

    #[test]
    fn withdraw_underflow_check() {
        const DEPOSIT_AMOUNT: Amount = 500;
        const WITHDRAW_AMOUNT: Amount = DEPOSIT_AMOUNT + 50;

        let mut balance = Balance::default();
        assert!(balance.credit(DEPOSIT_AMOUNT).is_ok());
        assert_eq!(balance.debit(WITHDRAW_AMOUNT).unwrap_err(), Underflow);
    }

    #[test]
    fn freeze_works() {
        const DEPOSIT_AMOUNT: Amount = 500;
        const FREEZE_AMOUNT: Amount = 100;

        let mut balance = Balance::default();
        assert!(balance.credit(DEPOSIT_AMOUNT).is_ok());
        assert!(balance.freeze(FREEZE_AMOUNT).is_ok());
        assert_eq!(balance.frozen, FREEZE_AMOUNT);
        assert_eq!(balance.available, DEPOSIT_AMOUNT - FREEZE_AMOUNT);
    }

    #[test]
    fn freeze_cannot_move_more_than_available() {
        const DEPOSIT_AMOUNT: Amount = 500;
        const FREEZE_AMOUNT: Amount = DEPOSIT_AMOUNT + 100;

        let mut balance = Balance::default();
        assert!(balance.credit(DEPOSIT_AMOUNT).is_ok());
        assert_eq!(balance.freeze(FREEZE_AMOUNT).unwrap_err(), Underflow);
    }

    #[test]
    fn unfreeze_works() {
        const DEPOSIT_AMOUNT: Amount = 500;
        const FREEZE_AMOUNT: Amount = 100;
        const UNFREEZE_AMOUNT: Amount = 80;

        let mut balance = Balance::default();
        assert!(balance.credit(DEPOSIT_AMOUNT).is_ok());
        assert!(balance.freeze(FREEZE_AMOUNT).is_ok());
        assert!(balance.unfreeze(UNFREEZE_AMOUNT).is_ok());
        assert_eq!(balance.frozen, FREEZE_AMOUNT - UNFREEZE_AMOUNT);
        assert_eq!(
            balance.available,
            DEPOSIT_AMOUNT - FREEZE_AMOUNT + UNFREEZE_AMOUNT
        );
    }

    #[test]
    fn unfreeze_cannot_move_more_than_frozen() {
        const DEPOSIT_AMOUNT: Amount = 500;
        const FREEZE_AMOUNT: Amount = 100;
        const UNFREEZE_AMOUNT: Amount = FREEZE_AMOUNT + 20;

        let mut balance = Balance::default();
        assert!(balance.credit(DEPOSIT_AMOUNT).is_ok());
        assert!(balance.freeze(FREEZE_AMOUNT).is_ok());
        assert_eq!(balance.unfreeze(UNFREEZE_AMOUNT).unwrap_err(), Underflow);
    }

    #[test]
    fn chargeback_works() {
        const DEPOSIT_AMOUNT: Amount = 500;

        let mut balance = Balance::default();
        assert!(balance.credit(DEPOSIT_AMOUNT).is_ok());
        assert!(balance.freeze(DEPOSIT_AMOUNT).is_ok());
        assert!(balance.chargeback(DEPOSIT_AMOUNT).is_ok());

        assert_eq!(balance.available, 0);
        assert_eq!(balance.frozen, 0);
    }

    #[test]
    fn chargeback_cannot_credit_more_than_frozen() {
        const DEPOSIT_AMOUNT: Amount = 500;
        const FREEZE_AMOUNT: Amount = DEPOSIT_AMOUNT - 100;

        let mut balance = Balance::default();
        assert!(balance.credit(DEPOSIT_AMOUNT).is_ok());
        assert!(balance.freeze(FREEZE_AMOUNT).is_ok());
        assert_eq!(balance.chargeback(DEPOSIT_AMOUNT).unwrap_err(), Underflow);
    }
}
