// Per-tab production lock: configuration changes on a production device are
// refused until the user unlocks the tab by typing the device name.
const unlocked = new Set<string>();

export const isUnlocked = (tabKey: string) => unlocked.has(tabKey);
export const unlock = (tabKey: string) => unlocked.add(tabKey);
export const lock = (tabKey: string) => unlocked.delete(tabKey);
