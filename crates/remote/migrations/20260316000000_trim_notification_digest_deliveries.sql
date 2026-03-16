DROP INDEX IF EXISTS idx_notification_digest_deliveries_user_id;

ALTER TABLE notification_digest_deliveries
DROP COLUMN IF EXISTS user_id;
