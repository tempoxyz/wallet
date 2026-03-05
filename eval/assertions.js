// Promptfoo assertion module for validating agent stream-json transcripts.
// Replicates the logic from validate.sh in JavaScript.
//
// Each exported function receives (output, context) where:
//   output      – raw stream-json transcript (multiple JSON lines)
//   context.vars – test variables including `expect` (JSON string)
//
// Return format: { pass: boolean, score: number, reason: string }

// ---------------------------------------------------------------------------
// Parsing helpers
// ---------------------------------------------------------------------------

function parseBashCommands(transcript) {
  if (!transcript || typeof transcript !== "string") return [];

  const commands = [];
  for (const line of transcript.split("\n")) {
    if (!line.includes("tool_use")) continue;
    let obj;
    try {
      obj = JSON.parse(line);
    } catch {
      continue;
    }

    const contents = obj?.message?.content;
    if (!Array.isArray(contents)) continue;

    for (const block of contents) {
      if (
        block.type !== "tool_use" ||
        (block.name !== "Bash" && block.name !== "bash")
      )
        continue;

      const raw = block.input?.cmd ?? block.input?.command;
      if (typeof raw !== "string") continue;

      // Collapse backslash-continuations and newlines into single line
      const collapsed = raw.replace(/\\\n\s*/g, " ").replace(/\n/g, " ");
      commands.push(collapsed);
    }
  }
  return commands;
}

function findPrestoCommands(bashCmds) {
  const re = /(^|&&|\|\||;|\|)\s*(presto|cargo\s+run)\s/;
  return bashCmds.filter((cmd) => re.test(cmd));
}

function findCurlCommands(bashCmds) {
  const re = /(^|\s|\/)curl(\s|$)/;
  return bashCmds.filter((cmd) => re.test(cmd));
}

// ---------------------------------------------------------------------------
// Trigger accuracy
// ---------------------------------------------------------------------------

function evaluateTrigger(prestoCmds, curlCmds, expect) {
  const reasons = [];
  let pass = true;

  const prestoExpect = expect?.presto;
  if (prestoExpect) {
    if (prestoExpect.should_invoke === true && prestoCmds.length === 0) {
      pass = false;
      reasons.push(
        "expected  tempo-walletinvocation but none found in Bash commands",
      );
    }
    if (prestoExpect.should_invoke === false && prestoCmds.length > 0) {
      pass = false;
      reasons.push(
        ` tempo-walletinvoked but should not have been (${prestoCmds.length} calls)`,
      );
    }
  }

  const curlExpect = expect?.curl;
  if (curlExpect && curlExpect.should_invoke !== undefined) {
    if (curlExpect.should_invoke === true && curlCmds.length === 0) {
      pass = false;
      reasons.push("expected curl invocation but none found");
    }
    if (curlExpect.should_invoke === false && curlCmds.length > 0) {
      pass = false;
      reasons.push(
        `curl invoked but should not have been (${curlCmds.length} calls)`,
      );
    }
  }

  return { pass, reasons };
}

// ---------------------------------------------------------------------------
// Usage correctness (only when  tempo-walletwas expected AND invoked)
// ---------------------------------------------------------------------------

function evaluateUsage(prestoCmds, expect) {
  const pe = expect?.presto;
  if (!pe || pe.should_invoke !== true || prestoCmds.length === 0) {
    return { pass: true, reasons: [] };
  }

  const reasons = [];
  let pass = true;
  const joined = prestoCmds.join("\n");

  // url_pattern
  if (pe.url_pattern) {
    const re = new RegExp(pe.url_pattern);
    if (!prestoCmds.some((cmd) => re.test(cmd))) {
      pass = false;
      reasons.push(`no  tempo-walletcommand matched url_pattern: ${pe.url_pattern}`);
    }
  }

  // method
  if (pe.method) {
    const methodRe = new RegExp(
      `(-X|--request)\\s+${pe.method}`,
      "i",
    );
    if (!prestoCmds.some((cmd) => methodRe.test(cmd))) {
      pass = false;
      reasons.push(`expected method ${pe.method} not found`);
    }
  }

  // has_flag
  if (pe.has_flag) {
    if (!prestoCmds.some((cmd) => cmd.includes(pe.has_flag))) {
      pass = false;
      reasons.push(`expected flag ${pe.has_flag} not found`);
    }
  }

  // argv_contains – at least ONE of the listed strings must appear
  if (Array.isArray(pe.argv_contains) && pe.argv_contains.length > 0) {
    const found = pe.argv_contains.some((needle) =>
      prestoCmds.some((cmd) => cmd.includes(needle)),
    );
    if (!found) {
      pass = false;
      reasons.push(
        `none of ${JSON.stringify(pe.argv_contains)} found in command`,
      );
    }
  }

  // json_checks – TODO: implement jq-style predicate evaluation
  if (Array.isArray(pe.json_checks) && pe.json_checks.length > 0) {
    // Intentionally skipped for now; would require a jq evaluator.
  }

  return { pass, reasons };
}

// ---------------------------------------------------------------------------
// Shared parse-expect helper
// ---------------------------------------------------------------------------

function parseExpect(context) {
  const raw = context?.vars?.expect;
  if (!raw) return {};
  if (typeof raw === "object") return raw;
  try {
    return JSON.parse(raw);
  } catch {
    return {};
  }
}

// ---------------------------------------------------------------------------
// Exported promptfoo assertion functions
// ---------------------------------------------------------------------------

function checkTrigger(output, context) {
  const bashCmds = parseBashCommands(output);
  const prestoCmds = findPrestoCommands(bashCmds);
  const curlCmds = findCurlCommands(bashCmds);
  const expect = parseExpect(context);

  const { pass, reasons } = evaluateTrigger(prestoCmds, curlCmds, expect);

  return {
    pass,
    score: pass ? 1.0 : 0.0,
    reason: pass
      ? "trigger accuracy: all checks passed"
      : `trigger accuracy failed: ${reasons.join("; ")}`,
  };
}

function checkUsage(output, context) {
  const bashCmds = parseBashCommands(output);
  const prestoCmds = findPrestoCommands(bashCmds);
  const expect = parseExpect(context);

  const { pass, reasons } = evaluateUsage(prestoCmds, expect);

  return {
    pass,
    score: pass ? 1.0 : 0.0,
    reason: pass
      ? "usage correctness: all checks passed"
      : `usage correctness failed: ${reasons.join("; ")}`,
  };
}

function validatePrestoUsage(output, context) {
  const bashCmds = parseBashCommands(output);
  const prestoCmds = findPrestoCommands(bashCmds);
  const curlCmds = findCurlCommands(bashCmds);
  const expect = parseExpect(context);

  const trigger = evaluateTrigger(prestoCmds, curlCmds, expect);
  const usage = evaluateUsage(prestoCmds, expect);

  const allReasons = [...trigger.reasons, ...usage.reasons];
  const pass = trigger.pass && usage.pass;

  return {
    pass,
    score: pass ? 1.0 : 0.0,
    reason: pass
      ? "all checks passed"
      : `failed: ${allReasons.join("; ")}`,
  };
}

module.exports = { validatePrestoUsage, checkTrigger, checkUsage };
