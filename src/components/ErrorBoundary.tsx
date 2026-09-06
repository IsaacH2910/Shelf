import { Component, type ErrorInfo, type ReactNode } from "react";

interface Props {
  children: ReactNode;
}

interface State {
  error: Error | null;
}

export class ErrorBoundary extends Component<Props, State> {
  state: State = { error: null };

  static getDerivedStateFromError(error: Error) {
    return { error };
  }

  componentDidCatch(error: Error, info: ErrorInfo) {
    console.error("Shelf render error", error, info.componentStack);
  }

  render() {
    if (this.state.error) {
      return (
        <div className="flex h-dvh flex-col items-center justify-center gap-3 bg-bg px-6 text-center">
          <p className="text-lg font-semibold text-text">Shelf could not render this view</p>
          <p className="max-w-md text-sm text-muted">{this.state.error.message}</p>
          <div className="flex items-center gap-2">
            <button
              className="rounded-lg bg-accent px-4 py-2 text-sm font-medium text-bg"
              onClick={() => {
                if (this.state.error?.name === "ReferenceError") {
                  window.location.reload();
                  return;
                }
                this.setState({ error: null });
              }}
            >
              Retry view
            </button>
            <button
              className="rounded-lg border border-border px-4 py-2 text-sm font-medium text-text"
              onClick={() => window.location.reload()}
            >
              Reload app
            </button>
          </div>
        </div>
      );
    }
    return this.props.children;
  }
}
