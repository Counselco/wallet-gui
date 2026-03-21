import sys

with open('/home/josep/chronx/crates/chronx-state/src/engine.rs', 'r') as f:
    content = f.read()

# Update DefaultRecord handler to also save the default record details
old_default = '''                let mut updated = loan;
                updated.status = LoanStatus::Defaulted { defaulted_at: *defaulted_at };
                self.db.save_loan(&updated)?;

                info!(loan_id = %hex::encode(loan_id),
                      missed_stage = %missed_stage_index,
                      days_overdue = %days_overdue,
                      "Loan default recorded");
                Ok(())
            }

            Action::LoanReinstatement'''

new_default = '''                let mut updated = loan;
                updated.status = LoanStatus::Defaulted { defaulted_at: *defaulted_at };
                self.db.save_loan(&updated)?;

                // Persist detailed default record
                let default_record = LoanDefaultRecord {
                    loan_id: *loan_id,
                    missed_stage_index: *missed_stage_index,
                    missed_amount_kx: *missed_amount_kx,
                    late_fees_accrued_kx: *late_fees_accrued_kx,
                    days_overdue: *days_overdue,
                    outstanding_balance_kx: *outstanding_balance_kx,
                    stages_remaining: *stages_remaining,
                    defaulted_at: *defaulted_at,
                    memo: memo.clone(),
                };
                self.db.save_loan_default(loan_id, &default_record)?;

                info!(loan_id = %hex::encode(loan_id),
                      missed_stage = %missed_stage_index,
                      days_overdue = %days_overdue,
                      "Loan default recorded");
                Ok(())
            }

            Action::LoanReinstatement'''

if old_default not in content:
    print("ERROR: DefaultRecord save block not found")
    sys.exit(1)

content = content.replace(old_default, new_default)

# Also add LoanDefaultRecord to the db import
old_import = '    LoanRecord, LoanStatus,\n};'
new_import = '    LoanRecord, LoanStatus, LoanDefaultRecord,\n};'
if old_import not in content:
    print("ERROR: LoanRecord import not found")
    sys.exit(1)
content = content.replace(old_import, new_import)

with open('/home/josep/chronx/crates/chronx-state/src/engine.rs', 'w') as f:
    f.write(content)

print("OK: DefaultRecord handler updated to save default record")
