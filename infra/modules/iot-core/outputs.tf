output "iot_endpoint" {
  description = "IoT Core data endpoint (ATS) for MQTT connections."
  value       = data.aws_iot_endpoint.data_ats.endpoint_address
}

output "thing_type_name" {
  description = "IoT thing type name for device registration."
  value       = aws_iot_thing_type.vehicle_device.name
}

output "device_policy_name" {
  description = "IoT policy name to attach to device certificates."
  value       = aws_iot_policy.device_policy.name
}

output "fleet_group_arns" {
  description = "Map of fleet ID to thing group ARN."
  value       = { for k, v in aws_iot_thing_group.fleet : k => v.arn }
}

output "iot_data_arn" {
  description = "ARN prefix for IoT data plane actions."
  value       = "arn:aws:iot:${var.region}:${var.account_id}"
}
