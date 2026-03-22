#!/usr/bin/env python3
"""
Patch engine.rs:
- Insert new match arms for LoanFlagPost, LoanAmendment, LoanVisibilityChange, LoanSummaryPost
  after the PrivacySendHigh match arm

Also patch error.rs if InvalidAction is missing.
"""
import sys

ENGINE_FILE = "/home/josep/chronx/crates/chronx-state/src/engine.rs"
ERROR_FILE = "/home/josep/chronx/crates/chronx-core/src/error.rs"

# ── Step 1: Check/patch error.rs ──
with open(ERROR_FILE, "r") as f:
    error_content = f.read()

if "InvalidAction" not in error_content:
    # Insert after FeatureNotActive
    error_content = error_content.replace(
        '    FeatureNotActive(String),',
        '    FeatureNotActive(String),\n\n    #[error("invalid action: {0}")]\n    InvalidAction(String),',
    )
    with open(ERROR_FILE, "w") as f:
        f.write(error_content)
    print("error.rs: Added InvalidAction variant")
else:
    print("error.rs: InvalidAction already exists, skipping")

# ── Step 2: Patch engine.rs ──
with open(ENGINE_FILE, "r") as f:
    lines = f.readlines()

original_count = len(lines)
print(f"engine.rs: {original_count} lines before patching")

# Find the PrivacySendHigh match arm closing `},`
# Pattern: `Action::PrivacySendHigh { .. } => {` ... `},`
privacy_high_start = None
for i, line in enumerate(lines):
    if "Action::PrivacySendHigh" in line:
        privacy_high_start = i
        break

if privacy_high_start is None:
    print("ERROR: Could not find Action::PrivacySendHigh match arm")
    sys.exit(1)

print(f"Found Action::PrivacySendHigh at line {privacy_high_start + 1}")

# Find the closing `},` of this match arm
# We need to track brace depth
brace_depth = 0
arm_close_line = None
for i in range(privacy_high_start, min(privacy_high_start + 20, len(lines))):
    for ch in lines[i]:
        if ch == '{':
            brace_depth += 1
        elif ch == '}':
            brace_depth -= 1
    if brace_depth == 0 and '}' in lines[i]:
        arm_close_line = i
        break

if arm_close_line is None:
    print("ERROR: Could not find closing of PrivacySendHigh match arm")
    sys.exit(1)

print(f"PrivacySendHigh match arm closes at line {arm_close_line + 1}")

new_arms = """\
            Action::LoanFlagPost { loan_id, flag, memo, supersedes } => {
                // Stub — flag posting requires full signing authority infrastructure
                return Err(ChronxError::FeatureNotActive(
                    "Loan flag posting is not yet enabled. Coming in next protocol version.".to_string()
                ));
            },
            Action::LoanAmendment { .. } => {
                return Err(ChronxError::FeatureNotActive(
                    "Loan amendments require dual-signature infrastructure. Coming in next protocol version.".to_string()
                ));
            },
            Action::LoanVisibilityChange { .. } => {
                return Err(ChronxError::FeatureNotActive(
                    "Credit history publication requires Foundation governance activation. Disabled by default.".to_string()
                ));
            },
            Action::LoanSummaryPost { .. } => {
                return Err(ChronxError::FeatureNotActive(
                    "Loan summary anchors are generated automatically at loan close. Not yet enabled.".to_string()
                ));
            },
"""

# Insert after the closing line of PrivacySendHigh
insert_at = arm_close_line + 1
new_arm_lines = [l + "\n" for l in new_arms.split("\n")]
for idx, nl in enumerate(new_arm_lines):
    lines.insert(insert_at + idx, nl)

with open(ENGINE_FILE, "w") as f:
    f.writelines(lines)

final_count = sum(1 for _ in open(ENGINE_FILE))
print(f"engine.rs: {final_count} lines after patching (added {final_count - original_count})")
print("SUCCESS: engine.rs patched")
