// Credential locker (src-tauri/src/credentials.rs). Profiles and defaults are
// plain data; every secret stays in the OS credential store and only crosses
// this boundary for the masked reveal/copy the user asks for.
import { invoke } from "@tauri-apps/api/core";
import { readText, writeText } from "@tauri-apps/plugin-clipboard-manager";
import type { Device } from "./types";

export type CredMethod = "password" | "key" | "agent";
export type CredSecretKind = "cred-password" | "cred-enable" | "cred-passphrase";

export interface Credential {
  id: string;
  name: string;
  username: string;
  method: CredMethod;
  keyPath: string;
  domain: string;
  hasPassword: boolean;
  hasEnable: boolean;
  hasPassphrase: boolean;
  tags: string[];
  notes: string;
  /** unix ms */
  updated: number;
  /** temporary: unix ms after which the secrets are deleted */
  expiresAt?: number | null;
}

export interface CredentialDefault {
  scope: "customer" | "site";
  value: string;
  credentialId: string;
}

export interface Locker {
  credentials: Credential[];
  defaults: CredentialDefault[];
  autoLockMinutes: number;
  clipboardClearSeconds: number;
  retryLimit: number;
}

export const EMPTY_LOCKER: Locker = { credentials: [], defaults: [], autoLockMinutes: 0, clipboardClearSeconds: 20, retryLimit: 3 };

export interface VaultStatus {
  configured: boolean;
  locked: boolean;
  autoLockMinutes: number;
}

export interface Effective {
  credentialId: string;
  name: string;
  via: "device" | "site" | "customer" | "own" | "none";
  username: string;
  hasPassword: boolean;
}

export interface TestResult {
  ok: boolean;
  message: string;
  millis: number;
}

export interface ImportSummary {
  added: number;
  updated: number;
  defaults: number;
}

export const loadLocker = () => invoke<Locker>("credentials_load");
export const saveLocker = (locker: Locker) => invoke("credentials_save", { locker });
export const setCredentialSecret = (kind: CredSecretKind, id: string, value: string) => invoke("credential_secret_set", { kind, id, value });
export const getCredentialSecret = (kind: CredSecretKind, id: string) => invoke<string | null>("credential_secret_get", { kind, id });
export const deleteCredential = (id: string) => invoke<Locker>("credential_delete", { id });
export const effectiveCredential = (deviceId: string) => invoke<Effective>("credential_effective", { deviceId });
export const exportCredentials = (passphrase: string) => invoke<string | null>("credentials_export", { passphrase });
export const importCredentials = (passphrase: string) => invoke<ImportSummary | null>("credentials_import", { passphrase });
export const testCredential = (credentialId: string, deviceId: string) => invoke<TestResult>("credential_test", { credentialId, deviceId });

export const vaultStatus = () => invoke<VaultStatus>("vault_status");
export const vaultSetup = (current: string | null, passphrase: string) => invoke<VaultStatus>("vault_setup", { current, passphrase });
export const vaultLock = () => invoke<VaultStatus>("vault_lock");
export const vaultUnlock = (passphrase: string) => invoke<VaultStatus>("vault_unlock", { passphrase });
export const vaultTouch = () => invoke("vault_touch");

export function newCredential(): Credential {
  return { id: crypto.randomUUID(), name: "New profile", username: "", method: "password", keyPath: "", domain: "", hasPassword: false, hasEnable: false, hasPassphrase: false, tags: [], notes: "", updated: Date.now(), expiresAt: null };
}

/** Which profile a device resolves to, following the backend's order: device, site default, customer default. */
export function resolveFor(locker: Locker, device: Device): { credential: Credential | null; via: Effective["via"] } {
  const now = Date.now();
  const alive = (c: Credential) => !c.expiresAt || c.expiresAt > now;
  const find = (id: string) => locker.credentials.find((c) => c.id === id && alive(c)) ?? null;
  if (device.credentialId) return { credential: find(device.credentialId), via: "device" };
  const by = (scope: CredentialDefault["scope"], value: string) => value ? locker.defaults.find((d) => d.scope === scope && d.value.toLowerCase() === value.toLowerCase()) : undefined;
  const site = by("site", device.site);
  if (site) return { credential: find(site.credentialId), via: "site" };
  const customer = by("customer", device.customer);
  if (customer) return { credential: find(customer.credentialId), via: "customer" };
  return { credential: null, via: "none" };
}

/** Devices that log in with this profile, directly or through a default. */
export function usedBy(locker: Locker, credential: Credential, devices: Device[]): { device: Device; via: Effective["via"] }[] {
  return devices.flatMap((d) => {
    const r = resolveFor(locker, d);
    return r.credential?.id === credential.id ? [{ device: d, via: r.via }] : [];
  });
}

let clearTimer: number | null = null;

/** Copy a secret and wipe the clipboard after `seconds`, unless something else was copied since. */
export async function copySecret(text: string, seconds: number): Promise<void> {
  await writeText(text);
  if (clearTimer) window.clearTimeout(clearTimer);
  clearTimer = null;
  if (seconds > 0) {
    clearTimer = window.setTimeout(async () => {
      try {
        if ((await readText()) === text) await writeText("");
      } catch {
        // clipboard unavailable: nothing to clear
      }
    }, seconds * 1000);
  }
}

export function expiryLabel(c: Credential): string {
  if (!c.expiresAt) return "";
  const left = c.expiresAt - Date.now();
  if (left <= 0) return "expired";
  const h = Math.round(left / 3_600_000);
  return h < 48 ? `expires in ${h} h` : `expires in ${Math.round(h / 24)} d`;
}
