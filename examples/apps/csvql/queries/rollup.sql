SELECT departments.name, count(*), min(employees.salary), max(employees.salary)
FROM employees
JOIN departments ON employees.dept_id = departments.id
GROUP BY departments.name
ORDER BY count(*) DESC
