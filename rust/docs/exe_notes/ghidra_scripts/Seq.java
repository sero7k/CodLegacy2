import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.*;
import ghidra.program.model.listing.*;
import ghidra.program.model.scalar.*;
import java.io.*;
import java.util.*;

// args: out stride count minStart  : functions whose operand scalars contain d, d+stride, ... (count terms)
public class Seq extends GhidraScript {
    @Override
    public void run() throws Exception {
        String[] a = getScriptArgs();
        PrintWriter pw = new PrintWriter(new FileWriter(a[0], false));
        int stride = Integer.parseInt(a[1]), count = Integer.parseInt(a[2]), minStart = Integer.parseInt(a[3]);
        Map<Function, Set<Long>> m = new LinkedHashMap<>();
        InstructionIterator it = currentProgram.getListing().getInstructions(true);
        while (it.hasNext() && !monitor.isCancelled()) {
            Instruction in = it.next();
            for (int op = 0; op < in.getNumOperands(); op++)
                for (Object o : in.getOpObjects(op))
                    if (o instanceof Scalar) {
                        long v = ((Scalar) o).getSignedValue();
                        if (v >= minStart && v < 0x40000) {
                            Function f = getFunctionContaining(in.getAddress());
                            if (f != null) m.computeIfAbsent(f, k -> new TreeSet<>()).add(v);
                        }
                    }
        }
        for (Map.Entry<Function, Set<Long>> e : m.entrySet()) {
            Set<Long> s = e.getValue();
            for (long d : s) {
                boolean ok = true;
                for (int i = 1; i < count; i++) if (!s.contains(d + (long) stride * i)) { ok = false; break; }
                if (ok) { pw.println(e.getKey().getEntryPoint() + " size=" + e.getKey().getBody().getNumAddresses() + " d=" + d + " (0x" + Long.toHexString(d) + ")"); }
            }
        }
        pw.close();
    }
}
