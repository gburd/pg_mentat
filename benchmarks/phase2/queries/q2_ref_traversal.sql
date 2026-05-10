-- Q2 ref traversal (EAV): issues assigned to user by email, with title + state.
-- Starts from email lookup (point), traverses assignee ref, joins title (text)
-- and state (keyword).
SELECT i.e, t.v, s.v
FROM eav.text    email
JOIN eav.ref     i ON i.a = 1005 AND i.v = email.e          -- assignee ref
JOIN eav.text    t ON t.e = i.e  AND t.a = 1002             -- title
JOIN eav.keyword s ON s.e = i.e  AND s.a = 1003             -- state
WHERE email.a = 1000 AND email.v = 'user100000@example.com'
