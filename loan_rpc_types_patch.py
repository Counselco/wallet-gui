import sys

with open('/home/josep/chronx/crates/chronx-rpc/src/types.rs', 'r') as f:
    content = f.read()

# Add loan RPC types at the end of the file
loan_types = '''

// ── Genesis 10a — Loan RPC types ─────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcLoanRecord {
    pub loan_id: String,
    pub lender: String,
    pub borrower: String,
    pub principal_kx: u64,
    pub pay_as: String,
    pub stages: Vec<RpcLoanPaymentStage>,
    pub prepayment: String,
    pub late_fee_schedule: String,
    pub grace_period_days: u8,
    pub hedge_requirement: Option<String>,
    pub oracle_policy: String,
    pub agreement_hash: Option<String>,
    pub status: String,
    pub created_at: u64,
    pub memo: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcLoanPaymentStage {
    pub stage_index: u32,
    pub due_at: u64,
    pub amount_kx: u64,
    pub pay_as: String,
    pub payment_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcLoanDefaultRecord {
    pub loan_id: String,
    pub missed_stage_index: u32,
    pub missed_amount_kx: u64,
    pub late_fees_accrued_kx: u64,
    pub days_overdue: u32,
    pub outstanding_balance_kx: u64,
    pub stages_remaining: u32,
    pub defaulted_at: u64,
    pub memo: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcOraclePrice {
    pub pair: String,
    pub spot_price_micro: u64,
    pub seven_day_avg_micro: u64,
    pub last_updated: u64,
    pub source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcLoanCounts {
    pub active: u64,
    pub defaulted: u64,
    pub completed: u64,
    pub written_off: u64,
    pub early_payoff: u64,
    pub reinstated: u64,
}
'''

content += loan_types

with open('/home/josep/chronx/crates/chronx-rpc/src/types.rs', 'w') as f:
    f.write(content)

print("OK: RPC loan types added to types.rs")
