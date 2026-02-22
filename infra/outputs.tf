output "iot_endpoint" {
  description = "AWS IoT Core data endpoint for MQTT connections."
  value       = module.iot_core.iot_endpoint
}

output "api_gateway_url" {
  description = "API Gateway invoke URL."
  value       = module.compute.api_gateway_url
}

output "db_endpoint" {
  description = "RDS PostgreSQL endpoint."
  value       = module.data.db_endpoint
  sensitive   = true
}

output "db_secret_arn" {
  description = "Secrets Manager ARN for database credentials."
  value       = module.data.db_secret_arn
}

output "vpc_id" {
  description = "VPC ID."
  value       = module.networking.vpc_id
}
