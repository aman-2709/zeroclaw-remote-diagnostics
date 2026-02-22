locals {
  prefix = "${var.project_name}-${var.environment}"
}

# ── Networking ──

module "networking" {
  source = "./modules/networking"

  prefix   = local.prefix
  vpc_cidr = var.vpc_cidr
  region   = var.aws_region
}

# ── IoT Core ──

module "iot_core" {
  source = "./modules/iot-core"

  prefix     = local.prefix
  region     = var.aws_region
  fleet_ids  = var.fleet_ids
  account_id = data.aws_caller_identity.current.account_id
}

# ── Data (RDS PostgreSQL) ──

module "data" {
  source = "./modules/data"

  prefix             = local.prefix
  vpc_id             = module.networking.vpc_id
  private_subnet_ids = module.networking.private_subnet_ids
  db_instance_class  = var.db_instance_class
  db_name            = var.db_name
  db_username        = var.db_username
}

# ── Compute (Lambda + API Gateway) ──

module "compute" {
  source = "./modules/compute"

  prefix             = local.prefix
  region             = var.aws_region
  api_stage_name     = var.api_stage_name
  vpc_id             = module.networking.vpc_id
  private_subnet_ids = module.networking.private_subnet_ids
  db_secret_arn      = module.data.db_secret_arn
  iot_endpoint       = module.iot_core.iot_endpoint
  iot_data_arn       = module.iot_core.iot_data_arn
}

# ── Monitoring ──

module "monitoring" {
  source = "./modules/monitoring"

  prefix               = local.prefix
  lambda_function_name = module.compute.lambda_function_name
  api_gateway_id       = module.compute.api_gateway_id
  api_stage_name       = var.api_stage_name
  db_instance_id       = module.data.db_instance_id
}

# ── Data Sources ──

data "aws_caller_identity" "current" {}
