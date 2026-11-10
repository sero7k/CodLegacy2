import ghidra.app.script.GhidraScript;
import ghidra.program.model.listing.*;
import java.util.*;
import java.util.regex.*;
import java.io.*;
// args: out minHits regex1 regex2 ... ; functions whose instruction texts match at least minHits of the regexes
public class InsnRegex extends GhidraScript {
  public void run() throws Exception {
    String[] a = getScriptArgs();
    int min = Integer.parseInt(a[1]);
    List<Pattern> ps = new ArrayList<>();
    for (int i = 2; i < a.length; i++) ps.add(Pattern.compile(a[i]));
    StringBuilder sb = new StringBuilder();
    FunctionIterator it = currentProgram.getFunctionManager().getFunctions(true);
    while (it.hasNext()) {
      Function f = it.next();
      boolean[] got = new boolean[ps.size()];
      InstructionIterator ii = currentProgram.getListing().getInstructions(f.getBody(), true);
      int n = 0;
      while (ii.hasNext() && n < 30000) { String t = ii.next().toString(); n++;
        for (int k = 0; k < ps.size(); k++) if (!got[k] && ps.get(k).matcher(t).find()) got[k] = true; }
      int c = 0; StringBuilder w = new StringBuilder();
      for (int k = 0; k < got.length; k++) if (got[k]) { c++; w.append(k); w.append(','); }
      if (c >= min) sb.append(String.format("%d %s %d [%s]\n", c, f.getEntryPoint(), n, w));
    }
    try (FileWriter fw = new FileWriter(a[0])) { fw.write(sb.toString()); }
    println("wrote "+a[0]);
  }
}
