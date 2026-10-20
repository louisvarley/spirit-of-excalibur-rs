// Read-only inspection of Turbo C sparse switches:
// MOV CX,N; MOV BX,valueTable; CMP AX,CS:[BX]; JE found; INC BX; INC BX; LOOP;
// JMP default; found: JMP word ptr CS:[BX + 2*N]
// @category AI

import java.util.regex.Matcher;
import java.util.regex.Pattern;

import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.Address;
import ghidra.program.model.listing.Instruction;
import ghidra.program.model.listing.InstructionIterator;
import ghidra.program.model.mem.MemoryBlock;

public class InspectSparseSwitch extends GhidraScript {

    private static final Pattern P = Pattern.compile(
            "\\bJMP\\s+word ptr CS:\\[([A-Z0-9]+)\\s*\\+\\s*(0x[0-9a-fA-F]+)\\]");
    private static final Pattern MOVI = Pattern.compile(
            "\\bMOV\\s+(CX|BX),\\s*(0x[0-9a-fA-F]+)");

    @Override
    protected void run() throws Exception {
        InstructionIterator it = currentProgram.getListing().getInstructions(true);
        int found = 0;
        while (it.hasNext() && !monitor.isCancelled()) {
            Instruction ins = it.next();
            Matcher m = P.matcher(ins.toString());
            if (!m.find()) {
                continue;
            }
            found++;
            int disp = Integer.parseInt(m.group(2).substring(2), 16);
            int n = -1;
            int tableOff = -1;
            Instruction cur = currentProgram.getListing().getInstructionBefore(ins.getAddress());
            boolean sawCmpLoop = false;
            for (int i = 0; i < 14 && cur != null; i++) {
                String t = cur.toString();
                if (t.startsWith("CMP AX,word ptr CS:[BX]")) {
                    sawCmpLoop = true;
                }
                Matcher mm = MOVI.matcher(t);
                if (mm.find()) {
                    if (mm.group(1).equals("CX") && n < 0) {
                        n = Integer.parseInt(mm.group(2).substring(2), 16);
                    }
                    else if (mm.group(1).equals("BX") && tableOff < 0) {
                        tableOff = Integer.parseInt(mm.group(2).substring(2), 16);
                    }
                }
                cur = currentProgram.getListing().getInstructionBefore(cur.getAddress());
            }
            println("SP " + ins.getAddress() + " " + ins + " disp=0x" + Integer.toHexString(disp)
                    + " N=" + n + " table=0x" + Integer.toHexString(tableOff)
                    + " cmpLoop=" + sawCmpLoop);
            if (n > 0 && tableOff >= 0 && disp == 2 * n) {
                MemoryBlock b = currentProgram.getMemory().getBlock(ins.getAddress());
                String seg = b.getStart().toString().split(":")[0];
                StringBuilder vals = new StringBuilder();
                for (int i = 0; i < n; i++) {
                    int v = currentProgram.getMemory().getShort(
                            toAddr(seg + ":" + String.format("%04x", tableOff + i * 2))) & 0xffff;
                    vals.append(String.format("%04x ", v));
                }
                println("  values: " + vals);
                StringBuilder tgs = new StringBuilder();
                for (int i = 0; i < n; i++) {
                    int v = currentProgram.getMemory().getShort(
                            toAddr(seg + ":" + String.format("%04x", tableOff + 2 * n + i * 2))) & 0xffff;
                    Address ta = toAddr(seg + ":" + String.format("%04x", v));
                    tgs.append(String.format("%s ", ta));
                }
                println("  targets: " + tgs);
            }
            else {
                println("  NOT SPARSE");
            }
        }
        println("SPARSE_TOTAL=" + found);
    }
}
