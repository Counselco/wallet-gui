import sys

with open('/home/josep/chronx/crates/chronx-rpc/src/server.rs', 'r') as f:
    content = f.read()

# 1. Add imports for new types
old_import = '    RpcDetailedTx, RpcActionSummary,\n};'
new_import = '    RpcDetailedTx, RpcActionSummary,\n    RpcLoanRecord, RpcLoanPaymentStage, RpcLoanDefaultRecord, RpcOraclePrice, RpcLoanCounts,\n};'
if old_import not in content:
    print("ERROR: import block not found in server.rs")
    sys.exit(1)
content = content.replace(old_import, new_import)

# Also add LoanStatus import from chronx_state::db
old_state_import = 'use chronx_state::db::{InvoiceStatus, CreditStatus, DepositStatus, ConditionalStatus};'
new_state_import = 'use chronx_state::db::{InvoiceStatus, CreditStatus, DepositStatus, ConditionalStatus, LoanStatus};'
if old_state_import not in content:
    print("ERROR: state db import not found in server.rs")
    sys.exit(1)
content = content.replace(old_state_import, new_state_import)

# 2. Add 6 RPC method implementations before the closing `}` of the impl block
# Insert after reject_invoice, before the closing `}`
old_impl_end = '''        Ok(tx_id)
    }
}


// ── Genesis 8 — RPC conversion helpers'''

new_impl_end = '''        Ok(tx_id)
    }

    // ── Genesis 10a — Loan queries ──────────────────────────────────────

    /// `chronx_getLoan` — fetch a single loan by its loan_id hex.
    async fn get_loan(&self, loan_id_hex: String) -> RpcResult<Option<RpcLoanRecord>> {
        let bytes = hex::decode(&loan_id_hex)
            .map_err(|e| rpc_err(-32602, format!("invalid hex: {e}")))?;
        if bytes.len() != 32 {
            return Err(rpc_err(-32602, "loan_id must be 32 bytes (64 hex chars)"));
        }
        let mut loan_id = [0u8; 32];
        loan_id.copy_from_slice(&bytes);
        let record = self.state.db.get_loan(&loan_id)
            .map_err(|e| rpc_err(-32603, e.to_string()))?;
        Ok(record.map(|r| loan_to_rpc(&r)))
    }

    /// `chronx_getLoansByWallet` — all loans for a given wallet (as lender or borrower).
    async fn get_loans_by_wallet(&self, wallet_address: String) -> RpcResult<Vec<RpcLoanRecord>> {
        let records = self.state.db.get_loans_by_wallet(&wallet_address)
            .map_err(|e| rpc_err(-32603, e.to_string()))?;
        Ok(records.iter().map(|r| loan_to_rpc(r)).collect())
    }

    /// `chronx_getLoanPaymentHistory` — return the payment stages for a loan.
    async fn get_loan_payment_history(&self, loan_id_hex: String) -> RpcResult<Vec<RpcLoanPaymentStage>> {
        let bytes = hex::decode(&loan_id_hex)
            .map_err(|e| rpc_err(-32602, format!("invalid hex: {e}")))?;
        if bytes.len() != 32 {
            return Err(rpc_err(-32602, "loan_id must be 32 bytes (64 hex chars)"));
        }
        let mut loan_id = [0u8; 32];
        loan_id.copy_from_slice(&bytes);
        let record = self.state.db.get_loan(&loan_id)
            .map_err(|e| rpc_err(-32603, e.to_string()))?;
        match record {
            Some(r) => Ok(r.stages.iter().map(|s| stage_to_rpc(s)).collect()),
            None => Err(rpc_err(-32602, format!("loan not found: {loan_id_hex}"))),
        }
    }

    /// `chronx_getLoanDefaultRecord` — return default record details for a loan.
    async fn get_loan_default_record(&self, loan_id_hex: String) -> RpcResult<Option<RpcLoanDefaultRecord>> {
        let bytes = hex::decode(&loan_id_hex)
            .map_err(|e| rpc_err(-32602, format!("invalid hex: {e}")))?;
        if bytes.len() != 32 {
            return Err(rpc_err(-32602, "loan_id must be 32 bytes (64 hex chars)"));
        }
        let mut loan_id = [0u8; 32];
        loan_id.copy_from_slice(&bytes);
        let record = self.state.db.get_loan_default(&loan_id)
            .map_err(|e| rpc_err(-32603, e.to_string()))?;
        Ok(record.map(|r| RpcLoanDefaultRecord {
            loan_id: hex::encode(r.loan_id),
            missed_stage_index: r.missed_stage_index,
            missed_amount_kx: r.missed_amount_kx,
            late_fees_accrued_kx: r.late_fees_accrued_kx,
            days_overdue: r.days_overdue,
            outstanding_balance_kx: r.outstanding_balance_kx,
            stages_remaining: r.stages_remaining,
            defaulted_at: r.defaulted_at,
            memo: r.memo,
        }))
    }

    /// `chronx_getOraclePrice` — return oracle price for a trading pair.
    async fn get_oracle_price_record(&self, pair: String) -> RpcResult<Option<RpcOraclePrice>> {
        let record = self.state.db.get_oracle_price(&pair)
            .map_err(|e| rpc_err(-32603, e.to_string()))?;
        Ok(record.map(|r| RpcOraclePrice {
            pair: r.pair,
            spot_price_micro: r.spot_price_micro,
            seven_day_avg_micro: r.seven_day_avg_micro,
            last_updated: r.last_updated,
            source: r.source,
        }))
    }

    /// `chronx_getActiveLoanCount` — aggregate loan counts by status.
    async fn get_active_loan_count(&self) -> RpcResult<RpcLoanCounts> {
        let all_loans = self.state.db.get_all_loans()
            .map_err(|e| rpc_err(-32603, e.to_string()))?;

        let mut active: u64 = 0;
        let mut defaulted: u64 = 0;
        let mut completed: u64 = 0;
        let mut written_off: u64 = 0;
        let mut early_payoff: u64 = 0;
        let mut reinstated: u64 = 0;

        for loan in &all_loans {
            match loan.status {
                LoanStatus::Active => active += 1,
                LoanStatus::Defaulted { .. } => defaulted += 1,
                LoanStatus::Completed { .. } => completed += 1,
                LoanStatus::WrittenOff { .. } => written_off += 1,
                LoanStatus::EarlyPayoff { .. } => early_payoff += 1,
                LoanStatus::Reinstated { .. } => reinstated += 1,
            }
        }

        Ok(RpcLoanCounts {
            active,
            defaulted,
            completed,
            written_off,
            early_payoff,
            reinstated,
        })
    }
}


// ── Genesis 8 — RPC conversion helpers'''

if old_impl_end not in content:
    print("ERROR: impl end block not found in server.rs")
    sys.exit(1)
content = content.replace(old_impl_end, new_impl_end)

# 3. Add helper converter functions at the end of the file
loan_helpers = '''

// ── Genesis 10a — Loan RPC conversion helpers ───────────────────────────────

fn loan_to_rpc(r: &chronx_state::db::LoanRecord) -> RpcLoanRecord {
    use chronx_core::transaction::{PayAsDenomination, PrepaymentTerms, LateFeeSchedule};

    let status = match &r.status {
        LoanStatus::Active => "Active".to_string(),
        LoanStatus::Defaulted { defaulted_at } => format!("Defaulted({})", defaulted_at),
        LoanStatus::Reinstated { reinstated_at } => format!("Reinstated({})", reinstated_at),
        LoanStatus::WrittenOff { written_off_at, outstanding_kx } =>
            format!("WrittenOff({},{}KX)", written_off_at, outstanding_kx),
        LoanStatus::Completed { completed_at } => format!("Completed({})", completed_at),
        LoanStatus::EarlyPayoff { paid_off_at } => format!("EarlyPayoff({})", paid_off_at),
    };

    let pay_as_str = match &r.pay_as {
        PayAsDenomination::FixedKX => "FixedKX".to_string(),
        PayAsDenomination::UsdEquivalentAtCreation { rate_microcents_per_kx } =>
            format!("UsdEquivalentAtCreation({})", rate_microcents_per_kx),
        PayAsDenomination::UsdEquivalentAtMaturity => "UsdEquivalentAtMaturity".to_string(),
        PayAsDenomination::EurEquivalentAtCreation { rate_microeuros_per_kx } =>
            format!("EurEquivalentAtCreation({})", rate_microeuros_per_kx),
        PayAsDenomination::EurEquivalentAtMaturity => "EurEquivalentAtMaturity".to_string(),
    };

    let prepayment_str = match &r.prepayment {
        PrepaymentTerms::Prohibited => "Prohibited".to_string(),
        PrepaymentTerms::AllowedAtPar => "AllowedAtPar".to_string(),
        PrepaymentTerms::AllowedWithPenalty { penalty_pct, penalty_minimum_kx } =>
            format!("AllowedWithPenalty({}%,{}KX)", penalty_pct, penalty_minimum_kx),
        PrepaymentTerms::AllowedWithDiscount { discount_pct, discount_maximum_kx } =>
            format!("AllowedWithDiscount({}%,{}KX)", discount_pct, discount_maximum_kx),
    };

    let late_fee_str = match &r.late_fee_schedule {
        LateFeeSchedule::None => "None".to_string(),
        LateFeeSchedule::Flat { fee_kx } => format!("Flat({}KX)", fee_kx),
        LateFeeSchedule::Tiered { stages } => format!("Tiered({} stages)", stages.len()),
    };

    let hedge_str = r.hedge_requirement.as_ref().map(|h| {
        format!("{}% {:?} in {}d", h.minimum_coverage_pct, h.coverage_type, h.funding_deadline_days)
    });

    let oracle_str = format!("retry_{}d_{}h_{:?}",
        r.oracle_policy.retry_window_days,
        r.oracle_policy.retry_interval_hours,
        r.oracle_policy.fallback,
    );

    RpcLoanRecord {
        loan_id: hex::encode(r.loan_id),
        lender: r.lender.clone(),
        borrower: r.borrower.clone(),
        principal_kx: r.principal_kx,
        pay_as: pay_as_str,
        stages: r.stages.iter().map(|s| stage_to_rpc(s)).collect(),
        prepayment: prepayment_str,
        late_fee_schedule: late_fee_str,
        grace_period_days: r.grace_period_days,
        hedge_requirement: hedge_str,
        oracle_policy: oracle_str,
        agreement_hash: r.agreement_hash.map(|h| hex::encode(h)),
        status,
        created_at: r.created_at,
        memo: r.memo.clone(),
    }
}

fn stage_to_rpc(s: &chronx_core::transaction::LoanPaymentStage) -> RpcLoanPaymentStage {
    use chronx_core::transaction::{PayAsDenomination, LoanPaymentType};

    let pay_as_str = match &s.pay_as {
        PayAsDenomination::FixedKX => "FixedKX".to_string(),
        PayAsDenomination::UsdEquivalentAtCreation { rate_microcents_per_kx } =>
            format!("UsdEquivalentAtCreation({})", rate_microcents_per_kx),
        PayAsDenomination::UsdEquivalentAtMaturity => "UsdEquivalentAtMaturity".to_string(),
        PayAsDenomination::EurEquivalentAtCreation { rate_microeuros_per_kx } =>
            format!("EurEquivalentAtCreation({})", rate_microeuros_per_kx),
        PayAsDenomination::EurEquivalentAtMaturity => "EurEquivalentAtMaturity".to_string(),
    };

    let payment_type_str = match &s.payment_type {
        LoanPaymentType::InterestOnly => "InterestOnly",
        LoanPaymentType::PrincipalOnly => "PrincipalOnly",
        LoanPaymentType::PrincipalAndInterest => "PrincipalAndInterest",
        LoanPaymentType::BulletFinal => "BulletFinal",
        LoanPaymentType::Custom => "Custom",
    };

    RpcLoanPaymentStage {
        stage_index: s.stage_index,
        due_at: s.due_at,
        amount_kx: s.amount_kx,
        pay_as: pay_as_str,
        payment_type: payment_type_str.to_string(),
    }
}
'''

content += loan_helpers

with open('/home/josep/chronx/crates/chronx-rpc/src/server.rs', 'w') as f:
    f.write(content)

print("OK: server.rs updated with loan RPC implementations")
