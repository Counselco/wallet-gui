import sys

with open('/home/josep/chronx/crates/chronx-rpc/src/api.rs', 'r') as f:
    content = f.read()

# First check what types are imported at the top
# Add after the last method before closing brace
old_end = '''    /// Submit a signed `RejectInvoice` transaction. `tx_hex` is hex-encoded bincode(Transaction).
    /// Returns the TxId hex on success. The transaction must contain exactly one
    /// `Action::RejectInvoice` action.
    #[method(name = "rejectInvoice")]
    async fn reject_invoice(&self, tx_hex: String) -> RpcResult<String>;
}'''

new_end = '''    /// Submit a signed `RejectInvoice` transaction. `tx_hex` is hex-encoded bincode(Transaction).
    /// Returns the TxId hex on success. The transaction must contain exactly one
    /// `Action::RejectInvoice` action.
    #[method(name = "rejectInvoice")]
    async fn reject_invoice(&self, tx_hex: String) -> RpcResult<String>;

    // ── Genesis 10a — Loan queries ──────────────────────────────────────

    /// Return a single loan record by loan_id (hex).
    #[method(name = "getLoan")]
    async fn get_loan(&self, loan_id_hex: String) -> RpcResult<Option<RpcLoanRecord>>;

    /// Return all loans where the given wallet is lender or borrower.
    #[method(name = "getLoansByWallet")]
    async fn get_loans_by_wallet(&self, wallet_address: String) -> RpcResult<Vec<RpcLoanRecord>>;

    /// Return payment stage status for a loan.
    #[method(name = "getLoanPaymentHistory")]
    async fn get_loan_payment_history(&self, loan_id_hex: String) -> RpcResult<Vec<RpcLoanPaymentStage>>;

    /// Return the default record for a loan, if one exists.
    #[method(name = "getLoanDefaultRecord")]
    async fn get_loan_default_record(&self, loan_id_hex: String) -> RpcResult<Option<RpcLoanDefaultRecord>>;

    /// Return oracle price for a trading pair.
    #[method(name = "getOraclePrice")]
    async fn get_oracle_price_record(&self, pair: String) -> RpcResult<Option<RpcOraclePrice>>;

    /// Return counts of loans by status.
    #[method(name = "getActiveLoanCount")]
    async fn get_active_loan_count(&self) -> RpcResult<RpcLoanCounts>;
}'''

if old_end not in content:
    print("ERROR: rejectInvoice end block not found in api.rs")
    sys.exit(1)

content = content.replace(old_end, new_end)

# Now add the new types to the imports at the top
# Find the existing use line for types
old_use = 'use super::types::{'
if old_use not in content:
    # Check if it uses a different import style
    print("WARNING: Could not find 'use super::types::{'")
    # Let's just print the first 20 lines
    for i, line in enumerate(content.split('\n')[:20]):
        print(f"{i}: {line}")
    sys.exit(1)

with open('/home/josep/chronx/crates/chronx-rpc/src/api.rs', 'w') as f:
    f.write(content)

print("OK: api.rs updated with loan RPC methods")
