// Read-only inspection of Turbo C CS:[reg+imm] switch tables.
// @category AI

import java.util.regex.Matcher;
import java.util.regex.Pattern;

import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.Address;
import ghidra.program.model.listing.Function;
import ghidra.program.model.listing.Instruction;
import ghidra.program.model.listing.InstructionIterator;
import ghidra.program.model.mem.MemoryBlock;

public class InspectCsSwitch extends GhidraScript {

    private static final Pattern P = Pattern.compile(
            "\\b(JMP|CALL)\\s+word ptr CS:\\[([A-Z0-9]+)\\s*\\+\\s*(0x[0-9a-fA-F]+)\\]");

    @Override
    protected void run() throws Exception {
        InstructionIterator it = currentProgram.getListing().getInstructions(true);
        int total = 0;
        while (it.hasNext() && !monitor.isCancelled()) {
            Instruction ins = it.next();
            String txt = ins.toString();
            Matcher m = P.matcher(txt);
            if (!m.find()) {
                continue;
            }
            total++;
            MemoryBlock block = currentProgram.getMemory().getBlock(ins.getAddress());
            String seg = block.getStart().toString().split(":")[0];
            int insOff = Integer.parseInt(ins.getAddress().toString().split(":")[1], 16);
            int tableOff = insOff + ins.getLength();
            Address table = toAddr(seg + ":" + String.format("%04x", tableOff));
            int entry0 = currentProgram.getMemory().getShort(table) & 0xffff;
            int n = (entry0 - tableOff) / 2;

            println("SW " + ins.getAddress() + " " + txt);
            println("  block=" + block.getName() + " seg=" + seg + " next=" + table
                    + " entry0=0x" + Integer.toHexString(entry0) + " N=" + n);

            // show preceding 8 instructions for the CMP/JBE bound check
            Address prev = ins.getAddress();
            for (int i = 0; i < 8; i++) {
                Instruction p = currentProgram.getListing().getInstructionBefore(prev);
                if (p == null) {
                    break;
                }
                println("    prev " + p.getAddress() + "  " + p);
                prev = p.getAddress();
            }

            if (n > 0 && n <= 2048) {
                StringBuilder sb = new StringBuilder();
                for (int i = 0; i < n; i++) {
                    Address e = table.add(i * 2L);
                    int v = currentProgram.getMemory().getShort(e) & 0xffff;
                    Address target = toAddr(seg + ":" + String.format("%04x", v));
                    Instruction ti = currentProgram.getListing().getInstructionAt(target);
                    Function tf = currentProgram.getFunctionManager().getFunctionContaining(target);
                    sb.append(String.format("    [%d] 0x%04x -> %s %s %s%n", i, v, target,
                            ti == null ? "UNDEF" : ti.toString(),
                            tf == null ? "" : "(in " + tf.getName() + ")"));
                }
                print(sb.toString());
            }
            else {
                println("  BAD N=" + n);
            }
        }
        println("TOTAL=" + total);
    }
}
