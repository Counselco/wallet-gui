import sys

with open('/home/josep/chronx/crates/chronx-state/src/engine.rs', 'r') as f:
    content = f.read()

# 1. Add CHRONOS_PER_KX to constants import
old_const = 'ORACLE_MIN_SUBMISSIONS, PROVIDER_BOND_CHRONOS, RECOVERY_CHALLENGE_WINDOW_SECS,\n    RECOVERY_EXECUTION_DELAY_SECS, RECOVERY_VERIFIER_THRESHOLD, SCHEMA_BOND_CHRONOS,\n};'
new_const = 'ORACLE_MIN_SUBMISSIONS, PROVIDER_BOND_CHRONOS, RECOVERY_CHALLENGE_WINDOW_SECS,\n    RECOVERY_EXECUTION_DELAY_SECS, RECOVERY_VERIFIER_THRESHOLD, SCHEMA_BOND_CHRONOS,\n    CHRONOS_PER_KX,\n};'
if old_const not in content:
    print("ERROR: constants import not found")
    sys.exit(1)
content = content.replace(old_const, new_const)

# 2. Add LoanRecord, LoanStatus to db import
old_db = '''use crate::db::{
    SignOfLifeRecord, PromiseChainRecord,
    StateDb,
    InvoiceRecord, InvoiceStatus,
    CreditRecord, CreditStatus,
    DepositRecord, DepositStatus,
    ConditionalRecord, ConditionalStatus,
    LedgerEntryRecord,
};'''
new_db = '''use crate::db::{
    SignOfLifeRecord, PromiseChainRecord,
    StateDb,
    InvoiceRecord, InvoiceStatus,
    CreditRecord, CreditStatus,
    DepositRecord, DepositStatus,
    ConditionalRecord, ConditionalStatus,
    LedgerEntryRecord,
    LoanRecord, LoanStatus,
};'''
if old_db not in content:
    print("ERROR: db import not found")
    sys.exit(1)
content = content.replace(old_db, new_db)

# 3. Add PrepaymentTerms to transaction import
old_tx = '    CreateLedgerEntryAction, LedgerEntryType,\n};'
new_tx = '    CreateLedgerEntryAction, LedgerEntryType,\n    PrepaymentTerms,\n};'
if old_tx not in content:
    print("ERROR: transaction import not found")
    sys.exit(1)
content = content.replace(old_tx, new_tx)

with open('/home/josep/chronx/crates/chronx-state/src/engine.rs', 'w') as f:
    f.write(content)

print("OK: imports added")
