variable "aws_region" {
  description = "AWS region for all resources."
  type        = string
  default     = "us-east-1"
}

variable "environment" {
  description = "Deployment environment (dev, staging, prod)."
  type        = string
  default     = "dev"

  validation {
    condition     = contains(["dev", "staging", "prod"], var.environment)
    error_message = "Environment must be dev, staging, or prod."
  }
}

variable "project_name" {
  description = "Project name used for resource naming."
  type        = string
  default     = "zeroclaw"
}

# ── Networking ──

variable "vpc_cidr" {
  description = "CIDR block for the VPC."
  type        = string
  default     = "10.0.0.0/16"
}

# ── Database ──

variable "db_instance_class" {
  description = "RDS instance class."
  type        = string
  default     = "db.t4g.micro"
}

variable "db_name" {
  description = "PostgreSQL database name."
  type        = string
  default     = "zeroclaw"
}

variable "db_username" {
  description = "PostgreSQL admin username."
  type        = string
  default     = "zcadmin"
}

# ── IoT Core ──

variable "fleet_ids" {
  description = "Fleet identifiers for IoT thing groups."
  type        = list(string)
  default     = ["fleet-alpha", "fleet-beta"]
}

variable "max_devices" {
  description = "Maximum number of devices (for capacity planning)."
  type        = number
  default     = 50
}

# ── API ──

variable "api_stage_name" {
  description = "API Gateway stage name."
  type        = string
  default     = "v1"
}
