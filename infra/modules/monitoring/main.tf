# CloudWatch dashboards, alarms, and log groups for observability.

# ── Lambda Alarms ──

resource "aws_cloudwatch_metric_alarm" "lambda_errors" {
  alarm_name          = "${var.prefix}-lambda-errors"
  alarm_description   = "Lambda function error rate exceeds threshold"
  comparison_operator = "GreaterThanThreshold"
  evaluation_periods  = 2
  metric_name         = "Errors"
  namespace           = "AWS/Lambda"
  period              = 300
  statistic           = "Sum"
  threshold           = 5
  treat_missing_data  = "notBreaching"

  dimensions = {
    FunctionName = var.lambda_function_name
  }

  tags = { Name = "${var.prefix}-lambda-errors" }
}

resource "aws_cloudwatch_metric_alarm" "lambda_duration" {
  alarm_name          = "${var.prefix}-lambda-duration"
  alarm_description   = "Lambda p95 duration exceeds 2s (Bedrock fallback budget)"
  comparison_operator = "GreaterThanThreshold"
  evaluation_periods  = 3
  metric_name         = "Duration"
  namespace           = "AWS/Lambda"
  period              = 300
  extended_statistic  = "p95"
  threshold           = 2000
  treat_missing_data  = "notBreaching"

  dimensions = {
    FunctionName = var.lambda_function_name
  }

  tags = { Name = "${var.prefix}-lambda-duration" }
}

# ── API Gateway Alarms ──

resource "aws_cloudwatch_metric_alarm" "api_5xx" {
  alarm_name          = "${var.prefix}-api-5xx"
  alarm_description   = "API Gateway 5xx error rate"
  comparison_operator = "GreaterThanThreshold"
  evaluation_periods  = 2
  metric_name         = "5xx"
  namespace           = "AWS/ApiGateway"
  period              = 300
  statistic           = "Sum"
  threshold           = 10
  treat_missing_data  = "notBreaching"

  dimensions = {
    ApiId = var.api_gateway_id
    Stage = var.api_stage_name
  }

  tags = { Name = "${var.prefix}-api-5xx" }
}

resource "aws_cloudwatch_metric_alarm" "api_latency" {
  alarm_name          = "${var.prefix}-api-latency"
  alarm_description   = "API Gateway p95 latency exceeds 2s"
  comparison_operator = "GreaterThanThreshold"
  evaluation_periods  = 3
  metric_name         = "IntegrationLatency"
  namespace           = "AWS/ApiGateway"
  period              = 300
  extended_statistic  = "p95"
  threshold           = 2000
  treat_missing_data  = "notBreaching"

  dimensions = {
    ApiId = var.api_gateway_id
    Stage = var.api_stage_name
  }

  tags = { Name = "${var.prefix}-api-latency" }
}

# ── RDS Alarms ──

resource "aws_cloudwatch_metric_alarm" "db_cpu" {
  alarm_name          = "${var.prefix}-db-cpu"
  alarm_description   = "RDS CPU utilization exceeds 80%"
  comparison_operator = "GreaterThanThreshold"
  evaluation_periods  = 3
  metric_name         = "CPUUtilization"
  namespace           = "AWS/RDS"
  period              = 300
  statistic           = "Average"
  threshold           = 80
  treat_missing_data  = "notBreaching"

  dimensions = {
    DBInstanceIdentifier = var.db_instance_id
  }

  tags = { Name = "${var.prefix}-db-cpu" }
}

resource "aws_cloudwatch_metric_alarm" "db_connections" {
  alarm_name          = "${var.prefix}-db-connections"
  alarm_description   = "RDS database connections exceeds threshold"
  comparison_operator = "GreaterThanThreshold"
  evaluation_periods  = 2
  metric_name         = "DatabaseConnections"
  namespace           = "AWS/RDS"
  period              = 300
  statistic           = "Average"
  threshold           = 80
  treat_missing_data  = "notBreaching"

  dimensions = {
    DBInstanceIdentifier = var.db_instance_id
  }

  tags = { Name = "${var.prefix}-db-connections" }
}

resource "aws_cloudwatch_metric_alarm" "db_free_storage" {
  alarm_name          = "${var.prefix}-db-free-storage"
  alarm_description   = "RDS free storage below 5GB"
  comparison_operator = "LessThanThreshold"
  evaluation_periods  = 2
  metric_name         = "FreeStorageSpace"
  namespace           = "AWS/RDS"
  period              = 300
  statistic           = "Average"
  threshold           = 5368709120
  treat_missing_data  = "notBreaching"

  dimensions = {
    DBInstanceIdentifier = var.db_instance_id
  }

  tags = { Name = "${var.prefix}-db-free-storage" }
}

# ── CloudWatch Dashboard ──

resource "aws_cloudwatch_dashboard" "main" {
  dashboard_name = "${var.prefix}-overview"

  dashboard_body = jsonencode({
    widgets = [
      {
        type   = "metric"
        x      = 0
        y      = 0
        width  = 12
        height = 6
        properties = {
          title = "Lambda Invocations & Errors"
          metrics = [
            ["AWS/Lambda", "Invocations", "FunctionName", var.lambda_function_name, { stat = "Sum" }],
            ["AWS/Lambda", "Errors", "FunctionName", var.lambda_function_name, { stat = "Sum", color = "#d62728" }],
          ]
          period = 300
          region = data.aws_region.current.name
        }
      },
      {
        type   = "metric"
        x      = 12
        y      = 0
        width  = 12
        height = 6
        properties = {
          title = "Lambda Duration (p50, p95, p99)"
          metrics = [
            ["AWS/Lambda", "Duration", "FunctionName", var.lambda_function_name, { stat = "p50" }],
            ["AWS/Lambda", "Duration", "FunctionName", var.lambda_function_name, { stat = "p95", color = "#ff7f0e" }],
            ["AWS/Lambda", "Duration", "FunctionName", var.lambda_function_name, { stat = "p99", color = "#d62728" }],
          ]
          period = 300
          region = data.aws_region.current.name
        }
      },
      {
        type   = "metric"
        x      = 0
        y      = 6
        width  = 12
        height = 6
        properties = {
          title = "API Gateway Requests & 5xx"
          metrics = [
            ["AWS/ApiGateway", "Count", "ApiId", var.api_gateway_id, "Stage", var.api_stage_name, { stat = "Sum" }],
            ["AWS/ApiGateway", "5xx", "ApiId", var.api_gateway_id, "Stage", var.api_stage_name, { stat = "Sum", color = "#d62728" }],
          ]
          period = 300
          region = data.aws_region.current.name
        }
      },
      {
        type   = "metric"
        x      = 12
        y      = 6
        width  = 12
        height = 6
        properties = {
          title = "RDS CPU & Connections"
          metrics = [
            ["AWS/RDS", "CPUUtilization", "DBInstanceIdentifier", var.db_instance_id, { stat = "Average" }],
            ["AWS/RDS", "DatabaseConnections", "DBInstanceIdentifier", var.db_instance_id, { stat = "Average", yAxis = "right" }],
          ]
          period = 300
          region = data.aws_region.current.name
        }
      },
    ]
  })
}

data "aws_region" "current" {}
