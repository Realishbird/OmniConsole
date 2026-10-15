// Per-tab state for the tools panel sections: switching console tabs swaps
// the state object, so each device keeps its own checks, workflow, chat and
// deploy progress, and work that is still running keeps writing to its own
// tab's state after the user switches away.
import { useCallback, useState } from "react";

const caches = new Map<string, Map<string, unknown>>();

export function useTabState<T extends object>(scope: string, tabKey: string, fresh: () => T): [T, (patch: Partial<T>) => void] {
  const [, setTick] = useState(0);
  let m = caches.get(scope);
  if (!m) {
    m = new Map();
    caches.set(scope, m);
  }
  if (!m.has(tabKey)) m.set(tabKey, fresh());
  const st = m.get(tabKey) as T;
  const update = useCallback(
    (patch: Partial<T>) => {
      const target = caches.get(scope)?.get(tabKey) as T | undefined;
      if (target) Object.assign(target, patch);
      setTick((t) => t + 1);
    },
    [scope, tabKey],
  );
  return [st, update];
}

