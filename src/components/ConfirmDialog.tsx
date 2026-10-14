interface Props {
  title: string;
  message: string;
  confirm: string;
  danger?: boolean;
  onResult: (ok: boolean) => void;
}

export default function ConfirmDialog({ title, message, confirm, danger, onResult }: Props) {
  return (
    <div className="overlay" onMouseDown={(e) => e.target === e.currentTarget && onResult(false)}>
      <div className="dialog" onKeyDown={(e) => e.key === "Escape" && onResult(false)}>
        <h3>{title}</h3>
        <p>{message}</p>
        <div className="dialog-actions">
          <button className="btn" onClick={() => onResult(false)}>Cancel</button>
          <button className={`btn ${danger ? "danger" : "primary"}`} autoFocus onClick={() => onResult(true)}>{confirm}</button>
        </div>
      </div>
    </div>
  );
}
