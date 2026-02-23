/** Mirrors zc-protocol device types. */

export type DeviceStatus = 'online' | 'offline' | 'error' | 'provisioning';

/** Serde-serialized Rust enum: simple variants are strings, Custom(s) is { custom: s }. */
export type HardwareType =
	| 'raspberry_pi4'
	| 'raspberry_pi5'
	| 'industrial_sbc'
	| { custom: string };

const HW_LABELS: Record<string, string> = {
	raspberry_pi4: 'RPi 4',
	raspberry_pi5: 'RPi 5',
	industrial_sbc: 'Industrial SBC'
};

/** Display a HardwareType value as a readable string. */
export function formatHardwareType(hw: HardwareType): string {
	if (typeof hw === 'string') return HW_LABELS[hw] ?? hw;
	if (typeof hw === 'object' && hw !== null && 'custom' in hw) return hw.custom;
	return String(hw);
}

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

export interface ProvisionDeviceRequest {
	device_id: string;
	fleet_id: string;
	hardware_type: string;
	vin?: string;
	metadata?: Record<string, unknown>;
}
