variable "prefix" {
  description = "Resource name prefix."
  type        = string
}

variable "vpc_id" {
  description = "VPC ID."
  type        = string
}

variable "private_subnet_ids" {
  description = "Private subnet IDs for DB placement."
  type        = list(string)
}

variable "db_instance_class" {
  description = "RDS instance class."
  type        = string
}

variable "db_name" {
  description = "PostgreSQL database name."
  type        = string
}

variable "db_username" {
  description = "PostgreSQL admin username."
  type        = string
}
