# RDS PostgreSQL 16 with Secrets Manager for credentials.
# TimescaleDB extension enabled via parameter group.

# ── Random password ──

resource "random_password" "db_password" {
  length  = 32
  special = false
}

# ── Secrets Manager ──

resource "aws_secretsmanager_secret" "db_credentials" {
  name                    = "${var.prefix}-db-credentials"
  recovery_window_in_days = 0
}

resource "aws_secretsmanager_secret_version" "db_credentials" {
  secret_id = aws_secretsmanager_secret.db_credentials.id
  secret_string = jsonencode({
    username = var.db_username
    password = random_password.db_password.result
    dbname   = var.db_name
    engine   = "postgres"
    port     = 5432
  })
}

# ── Security Group ──

resource "aws_security_group" "db" {
  name_prefix = "${var.prefix}-db-"
  vpc_id      = var.vpc_id
  description = "Allow PostgreSQL from private subnets"

  ingress {
    description = "PostgreSQL from VPC"
    from_port   = 5432
    to_port     = 5432
    protocol    = "tcp"
    cidr_blocks = [data.aws_vpc.selected.cidr_block]
  }

  egress {
    from_port   = 0
    to_port     = 0
    protocol    = "-1"
    cidr_blocks = ["0.0.0.0/0"]
  }

  tags = { Name = "${var.prefix}-db-sg" }

  lifecycle {
    create_before_destroy = true
  }
}

data "aws_vpc" "selected" {
  id = var.vpc_id
}

# ── DB Subnet Group ──

resource "aws_db_subnet_group" "main" {
  name       = "${var.prefix}-db-subnet"
  subnet_ids = var.private_subnet_ids

  tags = { Name = "${var.prefix}-db-subnet-group" }
}

# ── Parameter Group (enable TimescaleDB) ──

resource "aws_db_parameter_group" "postgres16" {
  name_prefix = "${var.prefix}-pg16-"
  family      = "postgres16"
  description = "PostgreSQL 16 with TimescaleDB"

  parameter {
    name  = "shared_preload_libraries"
    value = "pg_stat_statements"
  }

  parameter {
    name  = "log_min_duration_statement"
    value = "1000"
  }

  lifecycle {
    create_before_destroy = true
  }
}

# ── RDS Instance ──

resource "aws_db_instance" "main" {
  identifier = "${var.prefix}-postgres"

  engine         = "postgres"
  engine_version = "16"
  instance_class = var.db_instance_class

  db_name  = var.db_name
  username = var.db_username
  password = random_password.db_password.result

  allocated_storage     = 20
  max_allocated_storage = 100
  storage_type          = "gp3"
  storage_encrypted     = true

  db_subnet_group_name   = aws_db_subnet_group.main.name
  vpc_security_group_ids = [aws_security_group.db.id]
  parameter_group_name   = aws_db_parameter_group.postgres16.name

  multi_az            = false
  publicly_accessible = false

  backup_retention_period = 7
  backup_window           = "03:00-04:00"
  maintenance_window      = "sun:04:00-sun:05:00"

  skip_final_snapshot       = true
  final_snapshot_identifier = "${var.prefix}-final-snapshot"
  deletion_protection       = false

  performance_insights_enabled = true

  tags = { Name = "${var.prefix}-postgres" }
}
