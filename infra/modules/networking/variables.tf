variable "prefix" {
  description = "Resource name prefix."
  type        = string
}

variable "vpc_cidr" {
  description = "CIDR block for the VPC."
  type        = string
}

variable "region" {
  description = "AWS region."
  type        = string
}
