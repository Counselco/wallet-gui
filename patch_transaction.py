#!/usr/bin/env python3
"""
Patch transaction.rs:
1. Insert new type definitions after the last line (after default_true fn)
2. Insert new Action variants before the closing } of the Action enum
"""
import sys

TRANSACTION_FILE = "/home/Josep/chronx/crates/chronx-core/src/transaction.rs"

# Fix path case for Linux
TRANSACTION_FILE = "/home/josep/chronx/crates/chronx-core/src/transaction.rs"

with open(TRANSACTION_FILE, "r") as f:
    lines = f.readlines()

original_count = len(lines)
print(f"transaction.rs: {original_count} lines before patching")

# ── Step 1: Find the Action enum closing brace ──
# PrivacySendHigh is the last variant. After its closing `},` the next `}` closes the enum.
# We look for the pattern: line with just `}` after `PrivacySendHigh` block
action_enum_close_line = None
in_privacy_send_high = False
for i, line in enumerate(lines):
    if "PrivacySendHigh" in line and "Action::" not in line:
        in_privacy_send_high = True
    if in_privacy_send_high and line.strip() == "},":
        # Next line with just `}` is the enum close
        for j in range(i + 1, min(i + 5, len(lines))):
            if lines[j].strip() == "}":
                action_enum_close_line = j
                break
        break

if action_enum_close_line is None:
    print("ERROR: Could not find Action enum closing brace after PrivacySendHigh")
    sys.exit(1)

print(f"Action enum closes at line {action_enum_close_line + 1}")

# ── Step 2: New Action variants to insert ──
new_variants = """\
    /// Post a status flag on a loan. Signing rules enforced by engine.
    LoanFlagPost {
        loan_id: String,
        flag: LoanStatusFlag,
        memo: Option<String>,
        supersedes: Option<String>,
    },

    /// Amend loan terms. Requires both party signatures.
    LoanAmendment {
        loan_id: String,
        new_annual_rate_bps: Option<u32>,
        new_renewal_period_seconds: Option<u64>,
        new_exit_rights: Option<String>,
        amendment_memo: Option<String>,
        lender_signature: Vec<u8>,
        borrower_signature: Vec<u8>,
    },

    /// Change child chain visibility.
    LoanVisibilityChange {
        loan_id: String,
        new_visibility: ChainVisibility,
        reason_memo: Option<String>,
    },

    /// Summary anchor posted at loan close by engine.
    LoanSummaryPost {
        loan_id: String,
        anchor: LoanSummaryAnchor,
    },
"""

# Insert before the closing brace of Action enum
variant_lines = [l + "\n" for l in new_variants.split("\n")]
# Add a blank line before variants
lines.insert(action_enum_close_line, "\n")
action_enum_close_line += 1
for idx, vl in enumerate(variant_lines):
    lines.insert(action_enum_close_line + idx, vl)

# ── Step 3: New type definitions to append at end of file ──
new_types = """
// ── Loan Flag & Credit History Types ──────────────────────────────────────────

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum PenaltyType {
    Flat,
    Percentage,
    MonthsInterest,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct PrepaymentPenalty {
    pub penalty_type: PenaltyType,
    pub amount: f64,
    pub currency: String,
    pub applies_before: Option<u64>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum PaymentStatus {
    OnTime,
    Late(u32),
    Partial(f64),
    Missed,
    Prepaid,
    AutoPaid,
    AutoPayFailed,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum LoanStatusFlag {
    Offered, Active, PaidOff, EarlyExit, CollateralCalled, Expired, Withdrawn,
    Late, Delinquent, Default, Accelerated, WrittenOff, Reinstated, Settled, Forgiven, Transferred, Refinanced,
    Bankruptcy, BankruptcyDischarged, Insolvency,
    Disputed, LitigationPending, Judgment, Garnishment,
    Amended, Forbearance, Deferral,
    Frozen, UnderReview,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub enum ChainVisibility {
    #[default]
    Private,
    LenderPublished,
    BorrowerDisputed,
    MutuallySealed,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct LoanPaymentRecord {
    pub loan_id: String,
    pub period_number: u32,
    pub status: PaymentStatus,
    pub amount_paid_chronos: u64,
    pub amount_required_chronos: u64,
    pub paid_at: Option<i64>,
    pub periods_late: u32,
    pub memo: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct LoanSummaryAnchor {
    pub loan_id: String,
    pub total_payments: u32,
    pub on_time: u32,
    pub late_1_period: u32,
    pub late_2_period: u32,
    pub late_3_plus_period: u32,
    pub missed: u32,
    pub partial: u32,
    pub prepaid: u32,
    pub terminal_status: LoanStatusFlag,
    pub child_chain_hash: Vec<u8>,
    pub child_chain_visibility: ChainVisibility,
    pub summary_memo: Option<String>,
}
"""

# Append to end of file
lines.append(new_types)

with open(TRANSACTION_FILE, "w") as f:
    f.writelines(lines)

final_count = sum(1 for _ in open(TRANSACTION_FILE))
print(f"transaction.rs: {final_count} lines after patching (added {final_count - original_count})")
print("SUCCESS: transaction.rs patched")
