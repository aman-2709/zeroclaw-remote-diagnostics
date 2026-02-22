variable "prefix" {
  description = "Resource name prefix."
  type        = string
}

variable "lambda_function_name" {
  description = "Lambda function name for metrics."
  type        = string
}

variable "api_gateway_id" {
  description = "API Gateway HTTP API ID."
  type        = string
}

variable "api_stage_name" {
  description = "API Gateway stage name."
  type        = string
}

variable "db_instance_id" {
  description = "RDS instance identifier."
  type        = string
}
