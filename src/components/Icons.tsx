// Small stroke icons (24x24 grid), drawn in currentColor.
import type { ReactNode } from "react";

function Icon({ children }: { children: ReactNode }) {
  return (
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      {children}
    </svg>
  );
}

export const PlusIcon = () => <Icon><path d="M12 5v14M5 12h14" /></Icon>;
export const CloseIcon = () => <Icon><path d="M6 6l12 12M18 6L6 18" /></Icon>;
export const SidebarIcon = () => <Icon><rect x="3" y="4" width="18" height="16" rx="2" /><path d="M9 4v16" /></Icon>;
export const SunIcon = () => <Icon><circle cx="12" cy="12" r="4" /><path d="M12 2v2M12 20v2M4.9 4.9l1.4 1.4M17.7 17.7l1.4 1.4M2 12h2M20 12h2M4.9 19.1l1.4-1.4M17.7 6.3l1.4-1.4" /></Icon>;
export const MoonIcon = () => <Icon><path d="M21 12.8A9 9 0 1 1 11.2 3a7 7 0 0 0 9.8 9.8z" /></Icon>;
export const SaveIcon = () => <Icon><path d="M12 3v12M7 10l5 5 5-5M5 21h14" /></Icon>;
export const SearchIcon = () => <Icon><circle cx="11" cy="11" r="7" /><path d="M20 20l-3.5-3.5" /></Icon>;
export const ChevronIcon = ({ closed }: { closed?: boolean }) => (
  <svg className={`chev${closed ? " closed" : ""}`} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round">
    <path d="M6 9l6 6 6-6" />
  </svg>
);
export const UpIcon = () => <Icon><path d="M18 15l-6-6-6 6" /></Icon>;
export const DownIcon = () => <Icon><path d="M6 9l6 6 6-6" /></Icon>;
export const Logo = () => (
  <svg className="logo" viewBox="0 0 64 64" aria-hidden="true">
    <rect x="2" y="2" width="60" height="60" rx="14" fill="var(--accent)" />
    <path d="M17 22l11 10-11 10" fill="none" stroke="var(--accent-text)" strokeWidth="5" strokeLinecap="round" strokeLinejoin="round" />
    <path d="M32 44h15" stroke="var(--accent-text)" strokeWidth="5" strokeLinecap="round" />
  </svg>
);
