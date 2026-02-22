variable "prefix" {
  description = "Resource name prefix."
  type        = string
}

variable "region" {
  description = "AWS region."
  type        = string
}

variable "fleet_ids" {
  description = "Fleet identifiers for thing groups."
  type        = list(string)
}

variable "account_id" {
  description = "AWS account ID."
  type        = string
}
