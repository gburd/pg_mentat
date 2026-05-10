-- Q4 predicate (EAV): open issues with priority >= 4 (title included).
-- Filter on state first (selective), then priority range, then fetch title.
SELECT i.e, t.v, p.v
FROM eav.keyword s
JOIN eav.long    p ON p.e = s.e AND p.a = 1004 AND p.v >= 4
JOIN eav.text    t ON t.e = s.e AND t.a = 1002
JOIN (SELECT s.e AS e FROM eav.keyword s WHERE s.a = 1003 AND s.v = ':state/open') i
     ON i.e = s.e
WHERE s.a = 1003 AND s.v = ':state/open'
