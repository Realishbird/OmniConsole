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
export const MoreIcon = () => <Icon><circle cx="5" cy="12" r="1" /><circle cx="12" cy="12" r="1" /><circle cx="19" cy="12" r="1" /></Icon>;
export const StarIcon = ({ filled }: { filled?: boolean }) => (
  <svg viewBox="0 0 24 24" fill={filled ? "currentColor" : "none"} stroke="currentColor" strokeWidth="2" strokeLinejoin="round" aria-hidden="true">
    <path d="M12 3l2.8 5.7 6.2.9-4.5 4.4 1.1 6.2L12 17.3 6.4 20.2l1.1-6.2L3 9.6l6.2-.9z" />
  </svg>
);
export const SplitRightIcon = () => <Icon><rect x="3" y="4" width="18" height="16" rx="2" /><path d="M12 4v16" /></Icon>;
export const SplitDownIcon = () => <Icon><rect x="3" y="4" width="18" height="16" rx="2" /><path d="M3 12h18" /></Icon>;
export const SettingsIcon = () => <Icon><circle cx="12" cy="12" r="3" /><path d="M19.4 15a1.7 1.7 0 0 0 .3 1.8l.1.1a2 2 0 1 1-2.8 2.8l-.1-.1a1.7 1.7 0 0 0-1.8-.3 1.7 1.7 0 0 0-1 1.5V21a2 2 0 1 1-4 0v-.1a1.7 1.7 0 0 0-1.1-1.5 1.7 1.7 0 0 0-1.8.3l-.1.1a2 2 0 1 1-2.8-2.8l.1-.1a1.7 1.7 0 0 0 .3-1.8 1.7 1.7 0 0 0-1.5-1H3a2 2 0 1 1 0-4h.1a1.7 1.7 0 0 0 1.5-1.1 1.7 1.7 0 0 0-.3-1.8l-.1-.1a2 2 0 1 1 2.8-2.8l.1.1a1.7 1.7 0 0 0 1.8.3H9a1.7 1.7 0 0 0 1-1.5V3a2 2 0 1 1 4 0v.1a1.7 1.7 0 0 0 1 1.5 1.7 1.7 0 0 0 1.8-.3l.1-.1a2 2 0 1 1 2.8 2.8l-.1.1a1.7 1.7 0 0 0-.3 1.8V9a1.7 1.7 0 0 0 1.5 1H21a2 2 0 1 1 0 4h-.1a1.7 1.7 0 0 0-1.5 1z" /></Icon>;
export const CommandIcon = () => <Icon><path d="M4 17l6-6-6-6M12 19h8" /></Icon>;
export const BookmarkIcon = () => <Icon><path d="M6 3h12v18l-6-4-6 4z" /></Icon>;
export const RecordIcon = () => <svg viewBox="0 0 24 24" aria-hidden="true"><circle cx="12" cy="12" r="6" fill="currentColor" /></svg>;
export const ChecksIcon = () => <Icon><path d="M9 11l3 3L22 4" /><path d="M21 12v7a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h11" /></Icon>;
export const StopIcon = () => <Icon><rect x="6" y="6" width="12" height="12" rx="1" /></Icon>;
export const PlayIcon = () => <Icon><path d="M6 4l14 8-14 8z" /></Icon>;
export const FolderIcon = () => <Icon><path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v9a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z" /></Icon>;
export const RefreshIcon = () => <Icon><path d="M21 12a9 9 0 1 1-2.6-6.4" /><path d="M21 3v6h-6" /></Icon>;
export const TrashIcon = () => <Icon><path d="M3 6h18M8 6V4h8v2M6 6l1 14h10l1-14" /></Icon>;
export const DiffIcon = () => <Icon><path d="M7 3v12M3 7h8" /><path d="M17 9v12M13 17h8" /></Icon>;
