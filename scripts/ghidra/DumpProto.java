// Decompiles functions that reference given strings or external symbols, plus their callers (depth N).
// Args: <outfile> <depth> <needle1,needle2,...>   (needles match defined-string text or external symbol names)
import ghidra.app.script.GhidraScript;
import ghidra.app.decompiler.*;
import ghidra.program.model.address.*;
import ghidra.program.model.listing.*;
import ghidra.program.model.symbol.*;
import java.io.*;
import java.util.*;

public class DumpProto extends GhidraScript {
    @Override
    public void run() throws Exception {
        String[] a = getScriptArgs();
        File out = new File(a[0]);
        int depth = Integer.parseInt(a[1]);
        String[] needles = a[2].split(",");
        Set<Function> seeds = new LinkedHashSet<>();
        Listing L = currentProgram.getListing();
        ReferenceManager rm = currentProgram.getReferenceManager();

        DataIterator di = L.getDefinedData(true);
        while (di.hasNext()) {
            Data d = di.next();
            if (!d.hasStringValue()) continue;
            String v = d.getDefaultValueRepresentation();
            for (String n : needles) if (v.contains(n)) addRefs(d.getAddress(), seeds, rm);
        }
        for (Symbol s : currentProgram.getSymbolTable().getAllSymbols(true)) {
            for (String n : needles)
                if (s.getName().equals(n) || s.getName().startsWith(n + "@"))
                    addRefs(s.getAddress(), seeds, rm);
        }
        // thunks: follow functions named like needle
        for (Function f : currentProgram.getFunctionManager().getFunctions(true))
            for (String n : needles)
                if (f.getName().equals(n)) {
                    for (Reference r : rm.getReferencesTo(f.getEntryPoint())) {
                        Function c = currentProgram.getFunctionManager().getFunctionContaining(r.getFromAddress());
                        if (c != null) seeds.add(c);
                    }
                }

        Set<Function> all = new LinkedHashSet<>(seeds);
        Set<Function> frontier = new LinkedHashSet<>(seeds);
        for (int i = 0; i < depth; i++) {
            Set<Function> next = new LinkedHashSet<>();
            for (Function f : frontier)
                for (Function c : f.getCallingFunctions(monitor)) if (all.add(c)) next.add(c);
            frontier = next;
        }

        DecompInterface ifc = new DecompInterface();
        ifc.openProgram(currentProgram);
        try (PrintWriter w = new PrintWriter(new BufferedWriter(new FileWriter(out)))) {
            w.println("## seeds=" + seeds.size() + " total=" + all.size());
            for (Function f : all) {
                if (monitor.isCancelled()) break;
                DecompileResults r = ifc.decompileFunction(f, 90, monitor);
                w.println("\n// ---- " + f.getName() + " @ " + f.getEntryPoint() + (seeds.contains(f) ? "  [SEED]" : ""));
                w.println(r.decompileCompleted() ? r.getDecompiledFunction().getC() : "// failed");
            }
        }
    }

    private void addRefs(Address target, Set<Function> seeds, ReferenceManager rm) {
        for (Reference r : rm.getReferencesTo(target)) {
            Function c = currentProgram.getFunctionManager().getFunctionContaining(r.getFromAddress());
            if (c != null) seeds.add(c);
        }
    }
}
