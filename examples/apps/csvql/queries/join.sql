SELECT employees.name, departments.name, employees.salary
FROM employees
JOIN departments ON employees.dept_id = departments.id
WHERE employees.salary > 135000
ORDER BY employees.salary DESC
