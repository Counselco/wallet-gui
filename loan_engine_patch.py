import sys

# Read the file
with open('/home/josep/chronx/crates/chronx-state/src/engine.rs', 'r') as f:
    content = f.read()

# The placeholder block to replace
old_block = '''            // ── Genesis 10a — Loan actions (TODO: implement) ───────────────
            Action::LoanCreate { .. } => {
                // TODO: Genesis 10a loan creation logic
                Err(ChronxError::Other("LoanCreate not yet implemented".into()))
            }
            Action::DefaultRecord { .. } => {
                Err(ChronxError::Other("DefaultRecord not yet implemented".into()))
            }
            Action::LoanReinstatement { .. } => {
                Err(ChronxError::Other("LoanReinstatement not yet implemented".into()))
            }
            Action::LoanWriteOff { .. } => {
                Err(ChronxError::Other("LoanWriteOff not yet implemented".into()))
            }
            Action::LoanEarlyPayoff { .. } => {
                Err(ChronxError::Other("LoanEarlyPayoff not yet implemented".into()))
            }
            Action::LoanCompletion { .. } => {
                Err(ChronxError::Other("LoanCompletion not yet implemented".into()))
            }'''

new_block = '''            // ── Genesis 10a — Loan actions ──────────────────────────────────

            Action::LoanCreate {
                lender_wallet, borrower_wallet, principal_kx, pay_as,
                stages, grace_period_days, late_fee_schedule, prepayment,
                hedge_requirement, oracle_policy, agreement_hash, memo,
            } => {
                // Validate basic fields
                if *principal_kx == 0 { return Err(ChronxError::ZeroAmount); }
                if stages.is_empty() { return Err(ChronxError::InvalidLoanStages); }
                if *grace_period_days < 1 { return Err(ChronxError::InvalidGracePeriod); }

                // Stages must be ordered by due_at
                for w in stages.windows(2) {
                    if w[1].due_at <= w[0].due_at {
                        return Err(ChronxError::LoanStagesNotOrdered);
                    }
                }
                // No stage in the past
                for s in stages {
                    if (s.due_at as i64) < now {
                        return Err(ChronxError::LoanStagesInPast);
                    }
                }

                // The transaction sender must be the lender
                if sender.account_id != *lender_wallet {
                    return Err(ChronxError::AuthPolicyViolation);
                }

                // Check lender (sender) has enough balance
                let principal_chronos = (*principal_kx as u128) * (CHRONOS_PER_KX as u128);
                if sender.spendable_balance() < principal_chronos {
                    return Err(ChronxError::InsufficientBalance {
                        need: principal_chronos,
                        have: sender.spendable_balance(),
                    });
                }

                // Debit lender (sender)
                sender.balance -= principal_chronos;

                // Credit borrower (create if needed)
                let mut borrower = self.db.get_account(borrower_wallet)?.unwrap_or_else(|| {
                    Account::new(
                        borrower_wallet.clone(),
                        AuthPolicy::SingleSig {
                            public_key: chronx_core::types::DilithiumPublicKey(vec![]),
                        },
                    )
                });
                borrower.balance += principal_chronos;
                staged.accounts.push(borrower);

                // Generate deterministic loan_id from tx_id + lender + borrower
                let loan_id: [u8; 32] = {
                    let mut h = blake3::Hasher::new();
                    h.update(&tx_id.0);
                    h.update(&lender_wallet.0);
                    h.update(&borrower_wallet.0);
                    *h.finalize().as_bytes()
                };

                // Persist loan record
                let record = LoanRecord {
                    loan_id,
                    lender: lender_wallet.to_string(),
                    borrower: borrower_wallet.to_string(),
                    principal_kx: *principal_kx,
                    pay_as: pay_as.clone(),
                    stages: stages.clone(),
                    prepayment: prepayment.clone(),
                    late_fee_schedule: late_fee_schedule.clone(),
                    grace_period_days: *grace_period_days,
                    hedge_requirement: hedge_requirement.clone(),
                    oracle_policy: oracle_policy.clone(),
                    agreement_hash: *agreement_hash,
                    status: LoanStatus::Active,
                    created_at: now as u64,
                    memo: memo.clone(),
                };
                self.db.save_loan(&record)?;

                info!(loan_id = %hex::encode(loan_id),
                      lender = %lender_wallet, borrower = %borrower_wallet,
                      principal_kx = %principal_kx,
                      "Loan created");
                Ok(())
            }

            Action::DefaultRecord {
                loan_id, missed_stage_index, missed_amount_kx,
                late_fees_accrued_kx, days_overdue, outstanding_balance_kx,
                stages_remaining, defaulted_at, memo,
            } => {
                // Only MISAI executor may submit default records
                let misai_executor = self.db
                    .get_meta("misai_executor_wallet")?
                    .map(|b| String::from_utf8_lossy(&b).to_string())
                    .unwrap_or_default();
                if misai_executor.is_empty() || sender.account_id.to_string() != misai_executor {
                    return Err(ChronxError::MisaiOnlyAction);
                }

                let loan = self.db.get_loan(loan_id)?
                    .ok_or_else(|| ChronxError::LoanNotFound(hex::encode(loan_id)))?;
                match loan.status {
                    LoanStatus::Active | LoanStatus::Reinstated { .. } => {}
                    _ => return Err(ChronxError::LoanNotActive),
                }

                let mut updated = loan;
                updated.status = LoanStatus::Defaulted { defaulted_at: *defaulted_at };
                self.db.save_loan(&updated)?;

                info!(loan_id = %hex::encode(loan_id),
                      missed_stage = %missed_stage_index,
                      days_overdue = %days_overdue,
                      "Loan default recorded");
                Ok(())
            }

            Action::LoanReinstatement { loan_id, cure_amount_kx, new_stages, memo } => {
                let loan = self.db.get_loan(loan_id)?
                    .ok_or_else(|| ChronxError::LoanNotFound(hex::encode(loan_id)))?;
                match loan.status {
                    LoanStatus::Defaulted { .. } => {}
                    _ => return Err(ChronxError::LoanNotInDefault),
                }

                // Validate new stages
                if new_stages.is_empty() { return Err(ChronxError::InvalidLoanStages); }
                for w in new_stages.windows(2) {
                    if w[1].due_at <= w[0].due_at {
                        return Err(ChronxError::LoanStagesNotOrdered);
                    }
                }

                let mut updated = loan;
                updated.status = LoanStatus::Reinstated { reinstated_at: now as u64 };
                updated.stages = new_stages.clone();
                if let Some(m) = memo { updated.memo = Some(m.clone()); }
                self.db.save_loan(&updated)?;

                info!(loan_id = %hex::encode(loan_id), "Loan reinstated");
                Ok(())
            }

            Action::LoanWriteOff { loan_id, outstanding_balance_kx, write_off_date, memo } => {
                let loan = self.db.get_loan(loan_id)?
                    .ok_or_else(|| ChronxError::LoanNotFound(hex::encode(loan_id)))?;
                match loan.status {
                    LoanStatus::Defaulted { .. } => {}
                    _ => return Err(ChronxError::LoanNotInDefault),
                }

                // Only the lender (tx sender) may write off
                if sender.account_id.to_string() != loan.lender {
                    return Err(ChronxError::AuthPolicyViolation);
                }

                let mut updated = loan;
                updated.status = LoanStatus::WrittenOff {
                    written_off_at: *write_off_date,
                    outstanding_kx: *outstanding_balance_kx,
                };
                if let Some(m) = memo { updated.memo = Some(m.clone()); }
                self.db.save_loan(&updated)?;

                info!(loan_id = %hex::encode(loan_id), "Loan written off");
                Ok(())
            }

            Action::LoanEarlyPayoff { loan_id, payoff_amount_kx, memo } => {
                let loan = self.db.get_loan(loan_id)?
                    .ok_or_else(|| ChronxError::LoanNotFound(hex::encode(loan_id)))?;
                match loan.status {
                    LoanStatus::Active | LoanStatus::Reinstated { .. } => {}
                    _ => return Err(ChronxError::LoanNotActive),
                }

                // Check prepayment terms
                match loan.prepayment {
                    PrepaymentTerms::Prohibited => return Err(ChronxError::PrepaymentProhibited),
                    _ => {}
                }

                let mut updated = loan;
                updated.status = LoanStatus::EarlyPayoff { paid_off_at: now as u64 };
                if let Some(m) = memo { updated.memo = Some(m.clone()); }
                self.db.save_loan(&updated)?;

                info!(loan_id = %hex::encode(loan_id), payoff_kx = %payoff_amount_kx, "Loan early payoff");
                Ok(())
            }

            Action::LoanCompletion { loan_id, total_paid_kx, completion_date, stages_completed, memo } => {
                // Only MISAI executor may mark completion
                let misai_executor = self.db
                    .get_meta("misai_executor_wallet")?
                    .map(|b| String::from_utf8_lossy(&b).to_string())
                    .unwrap_or_default();
                if misai_executor.is_empty() || sender.account_id.to_string() != misai_executor {
                    return Err(ChronxError::MisaiOnlyAction);
                }

                let loan = self.db.get_loan(loan_id)?
                    .ok_or_else(|| ChronxError::LoanNotFound(hex::encode(loan_id)))?;
                match loan.status {
                    LoanStatus::Active | LoanStatus::Reinstated { .. } => {}
                    _ => return Err(ChronxError::LoanNotActive),
                }

                let mut updated = loan;
                updated.status = LoanStatus::Completed { completed_at: *completion_date };
                self.db.save_loan(&updated)?;

                info!(loan_id = %hex::encode(loan_id),
                      total_paid_kx = %total_paid_kx,
                      stages = %stages_completed,
                      "Loan completed");
                Ok(())
            }'''

if old_block not in content:
    print("ERROR: old_block not found in file")
    sys.exit(1)

content = content.replace(old_block, new_block)

with open('/home/josep/chronx/crates/chronx-state/src/engine.rs', 'w') as f:
    f.write(content)

print("OK: engine handlers replaced")
