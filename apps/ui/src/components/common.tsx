import { useEffect, useId, type ButtonHTMLAttributes, type PropsWithChildren, type ReactNode } from 'react';
import { AlertTriangle, Check, Info, LoaderCircle, X, XCircle } from 'lucide-react';
import type { ResultStatus, ToastMessage } from '../types';

type ButtonVariant = 'primary' | 'secondary' | 'ghost' | 'danger';

export function Button({
  children,
  variant = 'secondary',
  size = 'default',
  icon,
  className = '',
  ...props
}: ButtonHTMLAttributes<HTMLButtonElement> & {
  variant?: ButtonVariant;
  size?: 'default' | 'small' | 'icon';
  icon?: ReactNode;
}) {
  return (
    <button
      className={`button button--${variant} button--${size} ${className}`.trim()}
      type="button"
      {...props}
    >
      {icon}
      {children}
    </button>
  );
}

export function IconButton({ label, children, ...props }: PropsWithChildren<{ label: string }> & ButtonHTMLAttributes<HTMLButtonElement>) {
  return (
    <button className="icon-button" type="button" aria-label={label} title={label} {...props}>
      {children}
    </button>
  );
}

export function Toggle({
  checked,
  onChange,
  label,
  disabled = false,
}: {
  checked: boolean;
  onChange: (checked: boolean) => void;
  label: string;
  disabled?: boolean;
}) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={label}
      className={`toggle ${checked ? 'is-on' : ''}`}
      onClick={() => onChange(!checked)}
      disabled={disabled}
    >
      <span className="toggle__thumb" />
    </button>
  );
}

export function Badge({
  children,
  tone = 'neutral',
}: PropsWithChildren<{ tone?: 'neutral' | 'accent' | 'success' | 'warning' | 'danger' | 'purple' }>) {
  return <span className={`badge badge--${tone}`}>{children}</span>;
}

export function StatusIcon({ status, size = 18 }: { status: ResultStatus; size?: number }) {
  if (status === 'success') return <Check size={size} aria-hidden />;
  if (status === 'warning') return <AlertTriangle size={size} aria-hidden />;
  return <XCircle size={size} aria-hidden />;
}

export function Modal({
  open,
  onClose,
  title,
  description,
  children,
  size = 'medium',
}: PropsWithChildren<{
  open: boolean;
  onClose: () => void;
  title: string;
  description?: string;
  size?: 'small' | 'medium' | 'large';
}>) {
  const titleId = useId();
  useEffect(() => {
    if (!open) return;
    const handleKey = (event: KeyboardEvent) => {
      if (event.key === 'Escape' && !event.defaultPrevented) onClose();
    };
    window.addEventListener('keydown', handleKey);
    return () => window.removeEventListener('keydown', handleKey);
  }, [onClose, open]);

  if (!open) return null;
  return (
    <div className="modal-backdrop" role="presentation" onMouseDown={(event) => event.target === event.currentTarget && onClose()}>
      <section className={`modal modal--${size}`} role="dialog" aria-modal="true" aria-labelledby={titleId}>
        <header className="modal__header">
          <div>
            <h2 id={titleId}>{title}</h2>
            {description && <p>{description}</p>}
          </div>
          <IconButton label="닫기" onClick={onClose}>
            <X size={20} />
          </IconButton>
        </header>
        <div className="modal__body">{children}</div>
      </section>
    </div>
  );
}

export function EmptyState({ icon, title, description, action }: { icon: ReactNode; title: string; description: string; action?: ReactNode }) {
  return (
    <div className="empty-state">
      <div className="empty-state__icon">{icon}</div>
      <h3>{title}</h3>
      <p>{description}</p>
      {action}
    </div>
  );
}

export function LoadingScreen() {
  return (
    <main className="boot-screen" aria-live="polite">
      <div className="brand-mark brand-mark--large">K</div>
      <LoaderCircle className="spin" size={24} />
      <strong>KeyForge를 준비하는 중</strong>
      <span>입력 엔진과 설정을 확인하고 있습니다.</span>
    </main>
  );
}

export function ToastRegion({ toasts, dismiss }: { toasts: ToastMessage[]; dismiss: (id: string) => void }) {
  return (
    <div className="toast-region" aria-live="polite" aria-relevant="additions">
      {toasts.map((toast) => (
        <article key={toast.id} className={`toast toast--${toast.status}`}>
          <div className="toast__icon">
            <StatusIcon status={toast.status} />
          </div>
          <div className="toast__content">
            <strong>{toast.title}</strong>
            {toast.description && <p>{toast.description}</p>}
            {toast.actionLabel && toast.onAction && (
              <button type="button" className="toast__action" onClick={toast.onAction}>
                {toast.actionLabel}
              </button>
            )}
          </div>
          <IconButton label="알림 닫기" onClick={() => dismiss(toast.id)}>
            <X size={16} />
          </IconButton>
        </article>
      ))}
    </div>
  );
}

export function Callout({ tone = 'info', title, children }: PropsWithChildren<{ tone?: 'info' | 'warning' | 'danger'; title: string }>) {
  const Icon = tone === 'info' ? Info : tone === 'warning' ? AlertTriangle : XCircle;
  return (
    <div className={`callout callout--${tone}`}>
      <Icon size={18} aria-hidden />
      <div>
        <strong>{title}</strong>
        <div>{children}</div>
      </div>
    </div>
  );
}
