const { spawn } = require('child_process');
const fs = require('fs');
const os = require('os');
const path = require('path');

class AgentProvider {
  constructor(options) {
    this.agent = options.config?.agent || 'amp';
    this.timeout = options.config?.timeout || 180;
    this.providerId = options.id || `agent:${this.agent}`;
  }

  id() {
    return this.providerId;
  }

  async callApi(prompt) {
    const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), 'tempo-wallet-eval-'));

    try {
      let cmd, args, env;

      if (this.agent === 'claude') {
        cmd = 'claude';
        args = ['-p', '--verbose', '--dangerously-skip-permissions', '--output-format', 'stream-json', prompt];
        env = { ...process.env };
        delete env.CLAUDECODE;
      } else {
        cmd = 'amp';
        args = ['--stream-json', '-x', prompt];
        env = { ...process.env };
      }

      const output = await new Promise((resolve, reject) => {
        const chunks = [];
        const child = spawn(cmd, args, {
          cwd: tmpDir,
          env,
          stdio: ['pipe', 'pipe', 'pipe'],
        });

        child.stdin.end();
        child.stdout.on('data', (chunk) => chunks.push(chunk));

        const timer = setTimeout(() => {
          child.kill('SIGTERM');
          reject(new Error('timeout'));
        }, this.timeout * 1000);

        child.on('close', () => {
          clearTimeout(timer);
          resolve(Buffer.concat(chunks).toString('utf-8'));
        });

        child.on('error', (err) => {
          clearTimeout(timer);
          reject(err);
        });
      });

      return { output };
    } catch (err) {
      if (err.message === 'timeout') {
        return { error: 'timeout' };
      }
      return { error: err.message };
    } finally {
      fs.rmSync(tmpDir, { recursive: true, force: true });
    }
  }
}

module.exports = AgentProvider;
