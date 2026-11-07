// Own the child process, bounded output and cancellation lifecycle without invoking a shell.
import { spawn, type ChildProcessWithoutNullStreams } from 'node:child_process';
import path from 'node:path';
import { MIB } from './values';

export async function collectWorker(
  child: ChildProcessWithoutNullStreams,
  input: string,
  timeoutMs: number,
  signal?: AbortSignal,
): Promise<{ code: number | null; output: string }> {
  return new WorkerProcess(child, timeoutMs, signal).run(input);
}

class WorkerProcess {
  private readonly output: Buffer[] = [];
  private bytes = 0;
  private stderrBytes = 0;
  private settled = false;
  private failure?: Error;
  private timer?: NodeJS.Timeout;
  private killTimer?: NodeJS.Timeout;
  private resolve!: (value: { code: number | null; output: string }) => void;
  private reject!: (reason: Error) => void;
  private readonly abort = () => this.stop('Native operation was cancelled');
  private readonly exit = () => terminateTree(this.child);

  constructor(
    private readonly child: ChildProcessWithoutNullStreams,
    private readonly timeoutMs: number,
    private readonly signal?: AbortSignal,
  ) {}

  run(input: string): Promise<{ code: number | null; output: string }> {
    return new Promise((resolve, reject) => {
      this.resolve = resolve;
      this.reject = reject;
      this.timer = setTimeout(
        () => this.stop('Native operation exceeded its deadline'),
        this.timeoutMs,
      );
      this.signal?.addEventListener('abort', this.abort, { once: true });
      process.once('exit', this.exit);
      this.child.stdout.on('data', (chunk: Buffer) => this.collect(chunk));
      this.child.stderr.on('data', (chunk: Buffer) => {
        this.stderrBytes += chunk.length;
        if (this.stderrBytes > MIB) this.stop('Native diagnostic output exceeded its byte limit');
      });
      this.child.once('error', () => this.stop('Native worker could not start'));
      this.child.once('close', (code) => this.finish(code));
      this.child.stdin.on('error', () => this.stop('Native request could not be delivered'));
      if (this.signal?.aborted) this.abort();
      else this.child.stdin.end(input);
    });
  }

  private collect(chunk: Buffer): void {
    if (this.failure || this.settled) return;
    this.bytes += chunk.length;
    if (this.bytes > 32 * MIB) this.stop('Native response exceeded its byte limit');
    else this.output.push(chunk);
  }

  private stop(message: string): void {
    if (this.failure || this.settled) return;
    this.failure = new Error(message);
    terminateTree(this.child);
    this.killTimer = setTimeout(() => this.finish(null), 5000);
  }

  private finish(code: number | null): void {
    if (this.settled) return;
    this.settled = true;
    clearTimeout(this.timer);
    clearTimeout(this.killTimer);
    this.signal?.removeEventListener('abort', this.abort);
    process.removeListener('exit', this.exit);
    this.child.stdin.destroy();
    this.child.stdout.destroy();
    this.child.stderr.destroy();
    this.child.unref();
    if (this.failure) {
      this.reject(this.failure);
      return;
    }
    try {
      const output = new TextDecoder('utf-8', { fatal: true }).decode(
        Buffer.concat(this.output, this.bytes),
      );
      this.resolve({ code, output });
    } catch {
      this.reject(new Error('Native worker returned malformed UTF-8'));
    }
  }
}

function terminateTree(child: ChildProcessWithoutNullStreams): void {
  if (!child.pid) return;
  if (process.platform !== 'win32') {
    try {
      process.kill(-child.pid, 'SIGKILL');
    } catch {
      child.kill('SIGKILL');
    }
    return;
  }
  if (child.exitCode !== null || child.signalCode !== null) return;
  const executable = path.join(process.env.SystemRoot ?? 'C:\\Windows', 'System32', 'taskkill.exe');
  const killer = spawn(executable, ['/PID', String(child.pid), '/T', '/F'], {
    windowsHide: true,
    shell: false,
    stdio: 'ignore',
  });
  killer.once('error', () => child.kill());
  const timer = setTimeout(() => {
    killer.kill();
    child.kill();
  }, 4000);
  killer.once('close', () => clearTimeout(timer));
  killer.unref();
}
