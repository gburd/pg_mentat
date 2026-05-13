-- Q3 aggregate (EAV): count of issues per state.
-- Pure AEVT scan on keyword with GROUP BY.
SELECT v, COUNT(*) FROM eav.keyword WHERE a = 1003 GROUP BY v
