# AWS IoT Core — device registry, thing types, policies, topic rules.

# ── Thing Type ──

resource "aws_iot_thing_type" "vehicle_device" {
  name = "${var.prefix}-vehicle-device"

  properties {
    description           = "Connected vehicle diagnostic device (RPi/SBC)"
    searchable_attributes = ["fleet_id", "hardware_type"]
  }
}

# ── Thing Groups (per fleet) ──

resource "aws_iot_thing_group" "fleet" {
  for_each = toset(var.fleet_ids)

  name = "${var.prefix}-${each.value}"

  properties {
    description = "Fleet group: ${each.value}"

    attribute_payload {
      attributes = {
        fleet_id    = each.value
        environment = split("-", var.prefix)[1]
      }
    }
  }
}

# ── IoT Policy ──
# Scoped to device's own topics only (fleet/{fleet_id}/{device_id}/*).
# Uses policy variables for per-device scoping without per-device policies.

resource "aws_iot_policy" "device_policy" {
  name = "${var.prefix}-device-policy"

  policy = jsonencode({
    Version = "2012-10-17"
    Statement = [
      {
        Sid      = "AllowConnect"
        Effect   = "Allow"
        Action   = "iot:Connect"
        Resource = "arn:aws:iot:${var.region}:${var.account_id}:client/$${iot:Connection.Thing.ThingName}"
      },
      {
        Sid    = "AllowPublishOwnTopics"
        Effect = "Allow"
        Action = "iot:Publish"
        Resource = [
          "arn:aws:iot:${var.region}:${var.account_id}:topic/fleet/*/telemetry/*",
          "arn:aws:iot:${var.region}:${var.account_id}:topic/fleet/*/heartbeat",
          "arn:aws:iot:${var.region}:${var.account_id}:topic/fleet/*/commands/response",
          "arn:aws:iot:${var.region}:${var.account_id}:topic/fleet/*/commands/ack",
          "arn:aws:iot:${var.region}:${var.account_id}:topic/$$aws/things/$${iot:Connection.Thing.ThingName}/shadow/*",
        ]
      },
      {
        Sid    = "AllowSubscribeOwnTopics"
        Effect = "Allow"
        Action = "iot:Subscribe"
        Resource = [
          "arn:aws:iot:${var.region}:${var.account_id}:topicfilter/fleet/*/commands/request",
          "arn:aws:iot:${var.region}:${var.account_id}:topicfilter/fleet/*/config/*",
          "arn:aws:iot:${var.region}:${var.account_id}:topicfilter/fleet/broadcast/*",
          "arn:aws:iot:${var.region}:${var.account_id}:topicfilter/$$aws/things/$${iot:Connection.Thing.ThingName}/shadow/*",
        ]
      },
      {
        Sid    = "AllowReceiveOwnTopics"
        Effect = "Allow"
        Action = "iot:Receive"
        Resource = [
          "arn:aws:iot:${var.region}:${var.account_id}:topic/fleet/*/commands/request",
          "arn:aws:iot:${var.region}:${var.account_id}:topic/fleet/*/config/*",
          "arn:aws:iot:${var.region}:${var.account_id}:topic/fleet/broadcast/*",
          "arn:aws:iot:${var.region}:${var.account_id}:topic/$$aws/things/$${iot:Connection.Thing.ThingName}/shadow/*",
        ]
      },
      {
        Sid      = "DenyWriteECU"
        Effect   = "Deny"
        Action   = "iot:Publish"
        Resource = "arn:aws:iot:${var.region}:${var.account_id}:topic/fleet/*/commands/ecu-write"
      }
    ]
  })
}

# ── Topic Rule: Route telemetry to CloudWatch ──

resource "aws_iot_topic_rule" "telemetry_to_cloudwatch" {
  name        = replace("${var.prefix}_telemetry_log", "-", "_")
  description = "Route device telemetry to CloudWatch Logs"
  enabled     = true
  sql         = "SELECT * FROM 'fleet/+/+/telemetry/#'"
  sql_version = "2016-03-23"

  cloudwatch_logs {
    log_group_name = "/iot/${var.prefix}/telemetry"
    role_arn       = aws_iam_role.iot_rule_role.arn
  }
}

# ── Topic Rule: Route heartbeats to CloudWatch ──

resource "aws_iot_topic_rule" "heartbeat_to_cloudwatch" {
  name        = replace("${var.prefix}_heartbeat_log", "-", "_")
  description = "Route device heartbeats to CloudWatch Logs"
  enabled     = true
  sql         = "SELECT * FROM 'fleet/+/+/heartbeat'"
  sql_version = "2016-03-23"

  cloudwatch_logs {
    log_group_name = "/iot/${var.prefix}/heartbeat"
    role_arn       = aws_iam_role.iot_rule_role.arn
  }
}

# ── IAM Role for IoT Topic Rules ──

resource "aws_iam_role" "iot_rule_role" {
  name = "${var.prefix}-iot-rule-role"

  assume_role_policy = jsonencode({
    Version = "2012-10-17"
    Statement = [{
      Effect    = "Allow"
      Principal = { Service = "iot.amazonaws.com" }
      Action    = "sts:AssumeRole"
    }]
  })
}

resource "aws_iam_role_policy" "iot_rule_cloudwatch" {
  name = "${var.prefix}-iot-rule-cloudwatch"
  role = aws_iam_role.iot_rule_role.id

  policy = jsonencode({
    Version = "2012-10-17"
    Statement = [{
      Effect = "Allow"
      Action = [
        "logs:CreateLogGroup",
        "logs:CreateLogStream",
        "logs:PutLogEvents",
      ]
      Resource = "arn:aws:logs:${var.region}:${var.account_id}:log-group:/iot/${var.prefix}/*"
    }]
  })
}

# ── CloudWatch Log Groups for IoT ──

resource "aws_cloudwatch_log_group" "iot_telemetry" {
  name              = "/iot/${var.prefix}/telemetry"
  retention_in_days = 30
}

resource "aws_cloudwatch_log_group" "iot_heartbeat" {
  name              = "/iot/${var.prefix}/heartbeat"
  retention_in_days = 14
}

# ── IoT Endpoint (data source) ──

data "aws_iot_endpoint" "data_ats" {
  endpoint_type = "iot:Data-ATS"
}
