// For every indirect call through vtable offset +0x88 (send) or +0x8C (transact), scan backward in
// the same function for the two stack-buffer byte writes [+0]=len and [+1]=opcode feeding the call's
// argument, and print function @ callsite -> len/opcode. Best-effort heuristic.
// Args: <outfile>
import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.*;
import ghidra.program.model.listing.*;
import ghidra.program.model.pcode.*;
import ghidra.app.decompiler.*;
import java.io.*;
import java.util.regex.*;

public class OpcodeSites extends GhidraScript {
    @Override
    public void run() throws Exception {
        DecompInterface ifc = new DecompInterface();
        ifc.openProgram(currentProgram);
        Pattern call88 = Pattern.compile("\\+ 0x8[8c]\\)\\)\\(");
        // capture "xxx = 0xNN;" or "xxx[1] = 0xNN;" style constant byte writes
        Pattern byteWrite = Pattern.compile("(\\w+(?:\\[\\d+\\])?|\\*\\(undefined1 \\*\\)\\([^)]*\\)) = (0x[0-9a-fA-F]+|'[^']'|\\d+);");
        try (PrintWriter w = new PrintWriter(new BufferedWriter(new FileWriter(getScriptArgs()[0])))) {
            FunctionIterator it = currentProgram.getFunctionManager().getFunctions(true);
            while (it.hasNext() && !monitor.isCancelled()) {
                Function f = it.next();
                DecompileResults r = ifc.decompileFunction(f, 60, monitor);
                if (!r.decompileCompleted()) continue;
                String c = r.getDecompiledFunction().getC();
                if (!call88.matcher(c).find()) continue;
                String[] lines = c.split("\n");
                for (int i = 0; i < lines.length; i++) {
                    if (!call88.matcher(lines[i]).find()) continue;
                    // gather up to 25 preceding lines' constant byte writes
                    StringBuilder ctx = new StringBuilder();
                    for (int j = Math.max(0, i - 25); j < i; j++) {
                        Matcher m = byteWrite.matcher(lines[j]);
                        while (m.find()) ctx.append(m.group(1)).append("=").append(m.group(2)).append(" ");
                    }
                    w.println(f.getName() + " @ " + f.getEntryPoint() + "  call:" + lines[i].trim()
                            + "\n    writes: " + ctx);
                }
            }
        }
    }
}
