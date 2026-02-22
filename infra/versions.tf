terraform {
  required_version = ">= 1.5"

  required_providers {
    aws = {
      source  = "hashicorp/aws"
      version = "~> 5.0"
    }
  }

  # Remote state — uncomment when S3 backend is provisioned.
  # backend "s3" {
  #   bucket         = "zeroclaw-tfstate"
  #   key            = "remote-diagnostics/terraform.tfstate"
  #   region         = "us-east-1"
  #   dynamodb_table = "zeroclaw-tfstate-lock"
  #   encrypt        = true
  # }
}

provider "aws" {
  region = var.aws_region

  default_tags {
    tags = {
      Project     = "zeroclaw-remote-diagnostics"
      Environment = var.environment
      ManagedBy   = "terraform"
    }
  }
}
