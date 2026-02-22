variable "prefix" {
  description = "Resource name prefix."
  type        = string
}

variable "region" {
  description = "AWS region."
  type        = string
}

variable "api_stage_name" {
  description = "API Gateway stage name."
  type        = string
}

variable "vpc_id" {
  description = "VPC ID."
  type        = string
}

variable "private_subnet_ids" {
  description = "Private subnet IDs for Lambda placement."
  type        = list(string)
}

variable "db_secret_arn" {
  description = "Secrets Manager ARN for DB credentials."
  type        = string
}

variable "iot_endpoint" {
  description = "IoT Core data endpoint."
  type        = string
}

variable "iot_data_arn" {
  description = "ARN prefix for IoT data plane."
  type        = string
}
