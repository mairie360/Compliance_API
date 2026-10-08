-- Test fixtures run by the `seeder` service once Liquibase is done (dev, integration,
-- performance and security stacks).
--
-- User 1 (Admin) is already created by the `create_admin` changeset of the liquibase-migrations
-- image; it is the `sub` of the JWT ZAP injects. User 2 is a plain `User` account.
--
-- The password is the public argon2id hash of the template admin account: `users.password` only
-- accepts argon2id PHC strings since database 1.3.0 (chk_users_password_hashed).

INSERT INTO users (id, first_name, last_name, email, password, status, is_archived)
VALUES (
    2, 'Test', 'User', 'test2@mairie360.fr',
    '$argon2id$v=19$m=19456,t=2,p=1$/iKF9PbiDRDs4EKPjlIIhg$UKx9vfwwps250mEP/bYp63CXbEnQGULeUAhDq+az9Aw',
    'active', FALSE
)
ON CONFLICT (id) DO NOTHING;

INSERT INTO user_roles (user_id, role_id)
SELECT 2, id FROM roles WHERE name = 'User'
ON CONFLICT DO NOTHING;

-- Explicit ids do not advance the SERIAL sequence: move it past the fixtures so users created
-- later do not collide with them.
SELECT setval(pg_get_serial_sequence('users', 'id'), GREATEST((SELECT MAX(id) FROM users), 1));
