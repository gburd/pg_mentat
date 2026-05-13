-- Q1 point lookup (EAV): find user id + name by email.
-- Hits eav_text_aevt by a=1000 (email), then joins for name.
SELECT e.e, n.v
FROM eav.text e
JOIN eav.text n ON n.e = e.e AND n.a = 1001
WHERE e.a = 1000 AND e.v = 'user100000@example.com'
