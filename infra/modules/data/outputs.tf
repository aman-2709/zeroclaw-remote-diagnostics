output "db_endpoint" {
  description = "RDS PostgreSQL endpoint."
  value       = aws_db_instance.main.endpoint
}

output "db_instance_id" {
  description = "RDS instance identifier."
  value       = aws_db_instance.main.identifier
}

output "db_secret_arn" {
  description = "Secrets Manager ARN for DB credentials."
  value       = aws_secretsmanager_secret.db_credentials.arn
}

output "db_security_group_id" {
  description = "Security group ID for database access."
  value       = aws_security_group.db.id
}
