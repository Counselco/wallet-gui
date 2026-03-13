"""Fix: sender_email null display + amount trailing zeros in verified delivery email."""

NOTIFY_JS = '/opt/chronx-notify/index.js'

with open(NOTIFY_JS, 'r') as f:
    content = f.read()

# Fix 1: buildVerifiedRecipientEmail — guard senderEmail, format amount
old_fn_start = 'function buildVerifiedRecipientEmail(amount, senderEmail, memo) {'
new_fn_start = '''function buildVerifiedRecipientEmail(amount, senderEmail, memo) {
  const displaySender = (senderEmail && senderEmail !== 'null') ? senderEmail : 'Someone';
  const displayAmount = parseFloat(amount).toString();'''

content = content.replace(old_fn_start, new_fn_start, 1)

# Fix the template line that uses ${senderEmail} and ${amount}
old_line = "<p style=\"color:#eee;font-size:16px;margin:0 0 8px;\"><strong>${senderEmail}</strong> sent you <strong style=\"color:#C9A84C;\">${amount} KX</strong></p>"
new_line = "<p style=\"color:#eee;font-size:16px;margin:0 0 8px;\"><strong>${displaySender}</strong> sent you <strong style=\"color:#C9A84C;\">${displayAmount} KX</strong></p>"
content = content.replace(old_line, new_line, 1)

# Fix 2: The caller — pass sender_email with fallback, and format amount in subject
old_caller = '''          html = buildVerifiedRecipientEmail(amount, req.body.sender_email || null, memo);
          subject = `${amount} KX added to your ChronX wallet`;'''
new_caller = '''          html = buildVerifiedRecipientEmail(amount, sender_email || 'Someone', memo);
          subject = `${parseFloat(amount).toString()} KX added to your ChronX wallet`;'''
content = content.replace(old_caller, new_caller, 1)

with open(NOTIFY_JS, 'w') as f:
    f.write(content)

print('Patched buildVerifiedRecipientEmail + caller.')
