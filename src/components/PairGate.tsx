import { useState } from "react";
import { login } from "../lib/api";
import type { SessionInfo } from "../types";
import { Button } from "./Button";
import { Input } from "./Input";
import { PasswordInput } from "./PasswordInput";
import { Surface } from "./Surface";

export function LoginGate({ onSignedIn }: { onSignedIn: (session: SessionInfo) => void }) {
  const [username, setUsername] = useState("owner");
  const [password, setPassword] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  return (
    <div className="flex h-full items-center justify-center bg-bg px-6">
      <Surface className="w-full max-w-sm rounded-2xl p-6" padded={false}>
        <form
          onSubmit={async (e) => {
            e.preventDefault();
            setBusy(true);
            setError(null);
            try {
              const session = await login(username, password);
              onSignedIn(session);
            } catch (err) {
              setError(err instanceof Error ? err.message : "Could not sign in.");
            } finally {
              setBusy(false);
            }
          }}
        >
          <h1 className="text-xl font-semibold">Shelf</h1>
          <p className="mt-2 text-sm text-muted">
            Sign in with the owner username and password from the Mac app. There is no pairing PIN.
            Files stay on your Mac.
          </p>
          <label htmlFor="shelf-username" className="mt-5 block text-xs text-muted">
            Username
          </label>
          <Input
            id="shelf-username"
            autoComplete="username"
            autoCapitalize="none"
            spellCheck={false}
            value={username}
            onChange={(e) => setUsername(e.target.value)}
            className="mt-2"
          />
          <label htmlFor="shelf-password" className="mt-3 block text-xs text-muted">
            Password
          </label>
          <div className="mt-2">
            <PasswordInput
              id="shelf-password"
              autoComplete="current-password"
              value={password}
              onChange={(e) => setPassword(e.target.value)}
            />
          </div>
          {error && (
            <p className="mt-3 text-sm text-red-400" role="alert" aria-live="polite">
              {error}
            </p>
          )}
          <Button
            type="submit"
            disabled={busy || !username.trim() || !password}
            variant="primary"
            className="mt-4 flex w-full justify-center rounded-xl py-3"
          >
            {busy ? "Signing in…" : "Sign in"}
          </Button>
        </form>
      </Surface>
    </div>
  );
}
