/** Mirrors zc-protocol device types. */

export type DeviceStatus = 'online' | 'offline' | 'error' | 'provisioning';

export type HardwareType =
	| 'raspberry_pi_4'
	| 'raspberry_pi_5'
	| 'jetson_nano'
	| 'beaglebone'
	| 'custom_sbc'
	| 'unknown';

export interface DeviceSummary {
	device_id: string;
	status: DeviceStatus;
	hardware_type: HardwareType;
	last_heartbeat: string | null;
}

export interface DeviceInfo {
	id: string;
	fleet_id: string;
	device_id: string;
	status: DeviceStatus;
	vin: string | null;
	hardware_type: HardwareType;
	certificate_id: string | null;
	last_heartbeat: string | null;
	metadata: Record<string, unknown>;
	created_at: string;
	updated_at: string;
}
