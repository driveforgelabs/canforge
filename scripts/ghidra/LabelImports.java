// Labels IAT slots using a pe-sieve "*.imports.txt" file (lines: "<rva>,<addr>,<dll>.<func> #<ord>").
// Args: <imports.txt>
// Creates labels imp_<dll>_<func> at imageBase+rva so decompiled indirect calls become readable.
import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.Address;
import ghidra.program.model.symbol.SourceType;
import java.io.*;
import java.util.regex.*;

public class LabelImports extends GhidraScript {
    @Override
    public void run() throws Exception {
        File f = new File(getScriptArgs()[0]);
        Pattern p = Pattern.compile("^([0-9a-fA-F]+),[0-9a-fA-F]+,([^.\\s]+)\\.([^\\s#]+)");
        int n = 0;
        try (BufferedReader r = new BufferedReader(new FileReader(f))) {
            String line;
            while ((line = r.readLine()) != null) {
                Matcher m = p.matcher(line.trim());
                if (!m.find()) continue;
                long rva = Long.parseLong(m.group(1), 16);
                Address a = currentProgram.getImageBase().add(rva);
                String name = ("imp_" + m.group(2) + "_" + m.group(3)).replaceAll("[^A-Za-z0-9_]", "_");
                try {
                    createLabel(a, name, true, SourceType.USER_DEFINED);
                    n++;
                } catch (Exception e) {
                    println("skip " + a + " " + name + ": " + e.getMessage());
                }
            }
        }
        println("labelled " + n + " import slots");
    }
}
