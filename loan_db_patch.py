import sys

with open('/home/josep/chronx/crates/chronx-state/src/db.rs', 'r') as f:
    content = f.read()

# Add get_loans_by_wallet and get_all_loans after get_active_loans
old_end = '''    pub fn get_active_loans(&self) -> Result<Vec<LoanRecord>, ChronxError> {
        let mut loans = Vec::new();
        for item in self.loans.iter() {
            let (_, bytes) = item.map_err(|e| ChronxError::Storage(e.to_string()))?;
            let record: LoanRecord = bincode::deserialize(&bytes)
                .map_err(|e| ChronxError::Serialization(e.to_string()))?;
            match record.status {
                LoanStatus::Active | LoanStatus::Reinstated { .. } => loans.push(record),
                _ => {}
            }
        }
        Ok(loans)
    }
}'''

new_end = '''    pub fn get_active_loans(&self) -> Result<Vec<LoanRecord>, ChronxError> {
        let mut loans = Vec::new();
        for item in self.loans.iter() {
            let (_, bytes) = item.map_err(|e| ChronxError::Storage(e.to_string()))?;
            let record: LoanRecord = bincode::deserialize(&bytes)
                .map_err(|e| ChronxError::Serialization(e.to_string()))?;
            match record.status {
                LoanStatus::Active | LoanStatus::Reinstated { .. } => loans.push(record),
                _ => {}
            }
        }
        Ok(loans)
    }

    /// Return all loans where the given wallet is either lender or borrower.
    pub fn get_loans_by_wallet(&self, wallet: &str) -> Result<Vec<LoanRecord>, ChronxError> {
        let mut loans = Vec::new();
        for item in self.loans.iter() {
            let (_, bytes) = item.map_err(|e| ChronxError::Storage(e.to_string()))?;
            let record: LoanRecord = bincode::deserialize(&bytes)
                .map_err(|e| ChronxError::Serialization(e.to_string()))?;
            if record.lender == wallet || record.borrower == wallet {
                loans.push(record);
            }
        }
        Ok(loans)
    }

    /// Return all loans in the database.
    pub fn get_all_loans(&self) -> Result<Vec<LoanRecord>, ChronxError> {
        let mut loans = Vec::new();
        for item in self.loans.iter() {
            let (_, bytes) = item.map_err(|e| ChronxError::Storage(e.to_string()))?;
            let record: LoanRecord = bincode::deserialize(&bytes)
                .map_err(|e| ChronxError::Serialization(e.to_string()))?;
            loans.push(record);
        }
        Ok(loans)
    }

    /// Save a default record for a loan into the loan_defaults tree.
    pub fn save_loan_default(&self, loan_id: &[u8; 32], record: &LoanDefaultRecord) -> Result<(), ChronxError> {
        let bytes = bincode::serialize(record)
            .map_err(|e| ChronxError::Serialization(e.to_string()))?;
        self.loan_defaults.insert(loan_id, bytes)
            .map_err(|e| ChronxError::Storage(e.to_string()))?;
        Ok(())
    }

    /// Get the default record for a loan.
    pub fn get_loan_default(&self, loan_id: &[u8; 32]) -> Result<Option<LoanDefaultRecord>, ChronxError> {
        match self.loan_defaults.get(loan_id) {
            Ok(Some(bytes)) => {
                let record: LoanDefaultRecord = bincode::deserialize(&bytes)
                    .map_err(|e| ChronxError::Serialization(e.to_string()))?;
                Ok(Some(record))
            }
            Ok(None) => Ok(None),
            Err(e) => Err(ChronxError::Storage(e.to_string())),
        }
    }
}'''

if old_end not in content:
    print("ERROR: get_active_loans block not found in db.rs")
    sys.exit(1)

content = content.replace(old_end, new_end)

# Now add the LoanDefaultRecord struct after LoanRecord definition
# Find the OraclePriceRecord definition (which comes after LoanRecord)
old_oracle = '''#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct OraclePriceRecord {'''

new_oracle = '''#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct LoanDefaultRecord {
    pub loan_id: [u8; 32],
    pub missed_stage_index: u32,
    pub missed_amount_kx: u64,
    pub late_fees_accrued_kx: u64,
    pub days_overdue: u32,
    pub outstanding_balance_kx: u64,
    pub stages_remaining: u32,
    pub defaulted_at: u64,
    pub memo: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct OraclePriceRecord {'''

if old_oracle not in content:
    print("ERROR: OraclePriceRecord not found in db.rs")
    sys.exit(1)

content = content.replace(old_oracle, new_oracle)

with open('/home/josep/chronx/crates/chronx-state/src/db.rs', 'w') as f:
    f.write(content)

print("OK: db.rs updated with loan query methods and LoanDefaultRecord")
