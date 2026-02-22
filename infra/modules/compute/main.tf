# Lambda for command routing + API Gateway HTTP API.
# Lambda runs the zc-cloud-api binary (Rust, compiled for AL2023/ARM64).

# ── Lambda IAM Role ──

resource "aws_iam_role" "lambda_exec" {
  name = "${var.prefix}-lambda-exec"

  assume_role_policy = jsonencode({
    Version = "2012-10-17"
    Statement = [{
      Effect    = "Allow"
      Principal = { Service = "lambda.amazonaws.com" }
      Action    = "sts:AssumeRole"
    }]
  })
}

resource "aws_iam_role_policy_attachment" "lambda_basic" {
  role       = aws_iam_role.lambda_exec.name
  policy_arn = "arn:aws:iam::aws:policy/service-role/AWSLambdaBasicExecutionRole"
}

resource "aws_iam_role_policy_attachment" "lambda_vpc" {
  role       = aws_iam_role.lambda_exec.name
  policy_arn = "arn:aws:iam::aws:policy/service-role/AWSLambdaVPCAccessExecutionRole"
}

resource "aws_iam_role_policy" "lambda_custom" {
  name = "${var.prefix}-lambda-custom"
  role = aws_iam_role.lambda_exec.id

  policy = jsonencode({
    Version = "2012-10-17"
    Statement = [
      {
        Sid    = "SecretsManager"
        Effect = "Allow"
        Action = [
          "secretsmanager:GetSecretValue",
        ]
        Resource = var.db_secret_arn
      },
      {
        Sid    = "IoTPublish"
        Effect = "Allow"
        Action = [
          "iot:Publish",
          "iot:DescribeEndpoint",
        ]
        Resource = "${var.iot_data_arn}:topic/fleet/*"
      },
      {
        Sid    = "BedrockInvoke"
        Effect = "Allow"
        Action = [
          "bedrock:InvokeModel",
        ]
        Resource = "arn:aws:bedrock:${var.region}::foundation-model/*"
      },
    ]
  })
}

# ── Lambda Security Group ──

resource "aws_security_group" "lambda" {
  name_prefix = "${var.prefix}-lambda-"
  vpc_id      = var.vpc_id
  description = "Lambda function network access"

  egress {
    from_port   = 0
    to_port     = 0
    protocol    = "-1"
    cidr_blocks = ["0.0.0.0/0"]
  }

  tags = { Name = "${var.prefix}-lambda-sg" }

  lifecycle {
    create_before_destroy = true
  }
}

# ── Lambda Function ──
# Placeholder: actual deployment uses a CI/CD pipeline to upload the binary.
# The dummy zip is replaced on first deploy.

data "archive_file" "lambda_placeholder" {
  type        = "zip"
  output_path = "${path.module}/placeholder.zip"

  source {
    content  = "placeholder"
    filename = "bootstrap"
  }
}

resource "aws_lambda_function" "api" {
  function_name = "${var.prefix}-api"
  role          = aws_iam_role.lambda_exec.arn
  handler       = "bootstrap"
  runtime       = "provided.al2023"
  architectures = ["arm64"]
  timeout       = 30
  memory_size   = 256

  filename         = data.archive_file.lambda_placeholder.output_path
  source_code_hash = data.archive_file.lambda_placeholder.output_base64sha256

  vpc_config {
    subnet_ids         = var.private_subnet_ids
    security_group_ids = [aws_security_group.lambda.id]
  }

  environment {
    variables = {
      DATABASE_SECRET_ARN = var.db_secret_arn
      IOT_ENDPOINT        = var.iot_endpoint
      RUST_LOG            = "info,zc_cloud_api=debug"
    }
  }

  tags = { Name = "${var.prefix}-api-lambda" }

  lifecycle {
    ignore_changes = [filename, source_code_hash]
  }
}

# ── API Gateway HTTP API ──

resource "aws_apigatewayv2_api" "main" {
  name          = "${var.prefix}-api"
  protocol_type = "HTTP"
  description   = "ZeroClaw Remote Diagnostics API"

  cors_configuration {
    allow_headers = ["*"]
    allow_methods = ["GET", "POST", "PUT", "DELETE", "OPTIONS"]
    allow_origins = ["*"]
    max_age       = 3600
  }

  tags = { Name = "${var.prefix}-api-gw" }
}

resource "aws_apigatewayv2_integration" "lambda" {
  api_id                 = aws_apigatewayv2_api.main.id
  integration_type       = "AWS_PROXY"
  integration_uri        = aws_lambda_function.api.invoke_arn
  integration_method     = "POST"
  payload_format_version = "2.0"
}

resource "aws_apigatewayv2_route" "proxy" {
  api_id    = aws_apigatewayv2_api.main.id
  route_key = "$default"
  target    = "integrations/${aws_apigatewayv2_integration.lambda.id}"
}

resource "aws_apigatewayv2_stage" "main" {
  api_id      = aws_apigatewayv2_api.main.id
  name        = var.api_stage_name
  auto_deploy = true

  access_log_settings {
    destination_arn = aws_cloudwatch_log_group.api_gw.arn
    format = jsonencode({
      requestId          = "$context.requestId"
      ip                 = "$context.identity.sourceIp"
      requestTime        = "$context.requestTime"
      httpMethod         = "$context.httpMethod"
      routeKey           = "$context.routeKey"
      status             = "$context.status"
      protocol           = "$context.protocol"
      responseLength     = "$context.responseLength"
      integrationLatency = "$context.integrationLatency"
    })
  }
}

# ── Lambda Permission for API Gateway ──

resource "aws_lambda_permission" "api_gw" {
  statement_id  = "AllowAPIGateway"
  action        = "lambda:InvokeFunction"
  function_name = aws_lambda_function.api.function_name
  principal     = "apigateway.amazonaws.com"
  source_arn    = "${aws_apigatewayv2_api.main.execution_arn}/*/*"
}

# ── CloudWatch Log Group for API Gateway ──

resource "aws_cloudwatch_log_group" "api_gw" {
  name              = "/aws/apigateway/${var.prefix}-api"
  retention_in_days = 14
}
