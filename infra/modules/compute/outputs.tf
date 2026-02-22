output "lambda_function_name" {
  description = "Lambda function name."
  value       = aws_lambda_function.api.function_name
}

output "lambda_function_arn" {
  description = "Lambda function ARN."
  value       = aws_lambda_function.api.arn
}

output "api_gateway_id" {
  description = "API Gateway HTTP API ID."
  value       = aws_apigatewayv2_api.main.id
}

output "api_gateway_url" {
  description = "API Gateway invoke URL."
  value       = aws_apigatewayv2_stage.main.invoke_url
}
