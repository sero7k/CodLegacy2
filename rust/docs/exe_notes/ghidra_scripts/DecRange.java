import ghidra.app.script.GhidraScript;
import ghidra.app.decompiler.*;
import ghidra.program.model.listing.*;
import java.io.*;
import java.util.regex.*;

// args: outfile start end minSize maxSize regex ; decompiles functions in [start,end) and prints those whose C matches regex (all patterns separated by ';;' must match)
public class DecRange extends GhidraScript {
    @Override
    public void run() throws Exception {
        String[] a = getScriptArgs();
        PrintWriter pw = new PrintWriter(new FileWriter(a[0], false));
        long s = Long.decode(a[1]), e = Long.decode(a[2]);
        long mn = Long.decode(a[3]), mx = Long.decode(a[4]);
        String[] pats = a[5].split(";;");
        Pattern[] ps = new Pattern[pats.length];
        for (int i = 0; i < pats.length; i++) ps[i] = Pattern.compile(pats[i]);
        DecompInterface di = new DecompInterface();
        di.openProgram(currentProgram);
        FunctionIterator it = currentProgram.getFunctionManager().getFunctions(toAddr(s), true);
        int n = 0, hit = 0;
        while (it.hasNext() && !monitor.isCancelled()) {
            Function f = it.next();
            long ep = f.getEntryPoint().getOffset();
            if (ep >= e) break;
            long sz = f.getBody().getNumAddresses();
            if (sz < mn || sz > mx) continue;
            n++;
            DecompileResults r = di.decompileFunction(f, 30, monitor);
            if (r.getDecompiledFunction() == null) continue;
            String c = r.getDecompiledFunction().getC();
            boolean ok = true;
            for (Pattern p : ps) if (!p.matcher(c).find()) { ok = false; break; }
            if (ok) { hit++; pw.println("==== " + f.getEntryPoint() + " size " + sz); pw.println(c); pw.flush(); }
        }
        pw.println("scanned " + n + " hits " + hit);
        pw.close();
    }
}
