// Resolve Turbo C dense and sparse CS:[BX+imm] switch tables.
//
// Dense: JMP [BX+imm] immediately followed by N target words. Prefer a
//        preceding CMP AX,n bound (N=n+1) when all targets are in-segment;
//        otherwise use N=(entry0-tableBase)/2.
// Sparse: MOV CX,N; MOV BX,valueTable; CMP AX,CS:[BX]; JE found; INC BX x2;
//         LOOP; JMP default; found: JMP [BX+2*N]. valueTable holds N case
//         values followed by N target offsets.
// @category AI

import java.util.ArrayList;
import java.util.regex.Matcher;
import java.util.regex.Pattern;

import ghidra.app.cmd.function.CreateFunctionCmd;
import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.Address;
import ghidra.program.model.data.DataUtilities;
import ghidra.program.model.data.DataUtilities.ClearDataMode;
import ghidra.program.model.data.WordDataType;
import ghidra.program.model.listing.Function;
import ghidra.program.model.listing.Instruction;
import ghidra.program.model.listing.InstructionIterator;
import ghidra.program.model.mem.MemoryBlock;
import ghidra.program.model.pcode.JumpTable;
import ghidra.program.model.symbol.RefType;
import ghidra.program.model.symbol.Reference;
import ghidra.program.model.symbol.SourceType;

public class ResolveCsSwitchTables extends GhidraScript {

    private static final Pattern P = Pattern.compile(
            "\\b(JMP|CALL)\\s+word ptr CS:\\[([A-Z0-9]+)\\s*\\+\\s*(0x[0-9a-fA-F]+)\\]");
    private static final Pattern CMPAX = Pattern.compile(
            "\\bCMP\\s+AX,\\s*(0x[0-9a-fA-F]+)");
    private static final Pattern MOVI = Pattern.compile(
            "\\bMOV\\s+(CX|BX),\\s*(0x[0-9a-fA-F]+)");

    private int found, dense, sparse, skipped;

    private static class Sparse {
        int n;
        int valueTableOff;
    }

    @Override
    protected void run() throws Exception {
        InstructionIterator it = currentProgram.getListing().getInstructions(true);
        while (it.hasNext() && !monitor.isCancelled()) {
            Instruction ins = it.next();
            Matcher m = P.matcher(ins.toString());
            if (!m.find()) {
                continue;
            }
            found++;
            MemoryBlock block = currentProgram.getMemory().getBlock(ins.getAddress());
            if (block == null) {
                println("SKIP " + ins.getAddress() + " no memory block");
                skipped++;
                continue;
            }
            String seg = block.getStart().toString().split(":")[0];
            int insOff = Integer.parseInt(ins.getAddress().toString().split(":")[1], 16);
            int disp = Integer.parseInt(m.group(3).substring(2), 16);

            Sparse sp = findSparse(ins, disp);
            if (sp != null) {
                resolveSparse(ins, block, seg, sp);
            }
            else {
                resolveDense(ins, block, seg, insOff);
            }
        }
        println("SUMMARY found=" + found + " dense=" + dense + " sparse=" + sparse
                + " skipped=" + skipped);
    }

    private Sparse findSparse(Instruction ins, int disp) {
        int n = -1;
        int tableOff = -1;
        boolean cmpLoop = false;
        Instruction cur = currentProgram.getListing().getInstructionBefore(ins.getAddress());
        for (int i = 0; i < 16 && cur != null; i++) {
            String t = cur.toString();
            if (t.startsWith("CMP AX,word ptr CS:[BX]")) {
                cmpLoop = true;
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
        if (n > 0 && tableOff >= 0 && cmpLoop && disp == 2 * n) {
            Sparse s = new Sparse();
            s.n = n;
            s.valueTableOff = tableOff;
            return s;
        }
        return null;
    }

    private void resolveSparse(Instruction ins, MemoryBlock block, String seg, Sparse sp) throws Exception {
        int n = sp.n;
        int tableOff = sp.valueTableOff;
        int targetOff = tableOff + 2 * n;
        Address end = toAddr(seg + ":" + String.format("%04x", tableOff + 4 * n - 1));
        if (block.getStart().compareTo(toAddr(seg + ":" + String.format("%04x", tableOff))) > 0
                || block.getEnd().compareTo(end) < 0) {
            println("SKIP-SPARSE " + ins.getAddress() + " N=" + n + " table outside block");
            skipped++;
            return;
        }

        ArrayList<Address> targets = new ArrayList<>();
        for (int i = 0; i < n; i++) {
            int v = currentProgram.getMemory().getShort(
                    toAddr(seg + ":" + String.format("%04x", targetOff + i * 2))) & 0xffff;
            Address t = toAddr(seg + ":" + String.format("%04x", v));
            if (block.getStart().compareTo(t) > 0 || block.getEnd().compareTo(t) < 0) {
                println("SKIP-SPARSE " + ins.getAddress() + " target outside block: " + t);
                skipped++;
                return;
            }
            targets.add(t);
        }

        // Case-value words then target words.
        for (int i = 0; i < 2 * n; i++) {
            Address a = toAddr(seg + ":" + String.format("%04x", tableOff + i * 2));
            DataUtilities.createData(currentProgram, a, new WordDataType(), -1,
                    ClearDataMode.CLEAR_ALL_CONFLICT_DATA);
        }
        finish(ins, targets, "SPARSE N=" + n + " valueTable=0x"
                + Integer.toHexString(tableOff));
        sparse++;
    }

    private void resolveDense(Instruction ins, MemoryBlock block, String seg, int insOff) throws Exception {
        int tableOff = insOff + ins.getLength();
        Address table = toAddr(seg + ":" + String.format("%04x", tableOff));
        Address tableEnd = toAddr(seg + ":" + String.format("%04x", tableOff + 1));
        if (block.getStart().compareTo(table) > 0 || block.getEnd().compareTo(tableEnd) < 0) {
            println("SKIP " + ins.getAddress() + " table outside block");
            skipped++;
            return;
        }
        int entry0 = currentProgram.getMemory().getShort(table) & 0xffff;
        int formulaN = (entry0 - tableOff) / 2;
        int bound = findBound(ins.getAddress());

        int useN = -1;
        String how = "";
        if (bound >= 0 && bound > 0 && validEntries(block, seg, tableOff, bound + 1)) {
            useN = bound + 1;
            how = "bound=" + bound;
        }
        else if (formulaN > 0 && formulaN <= 512
                && validEntries(block, seg, tableOff, formulaN)) {
            useN = formulaN;
            how = "entry0";
        }
        if (useN < 0) {
            println("SKIP-DENSE " + ins.getAddress() + " formulaN=" + formulaN
                    + " bound=" + bound + " (no valid table size)");
            skipped++;
            return;
        }

        ArrayList<Address> targets = new ArrayList<>();
        for (int i = 0; i < useN; i++) {
            int v = currentProgram.getMemory().getShort(
                    toAddr(seg + ":" + String.format("%04x", tableOff + i * 2))) & 0xffff;
            targets.add(toAddr(seg + ":" + String.format("%04x", v)));
        }
        for (int i = 0; i < useN; i++) {
            Address a = toAddr(seg + ":" + String.format("%04x", tableOff + i * 2));
            DataUtilities.createData(currentProgram, a, new WordDataType(), -1,
                    ClearDataMode.CLEAR_ALL_CONFLICT_DATA);
        }
        finish(ins, targets, "DENSE N=" + useN + " (" + how + ") table=0x"
                + Integer.toHexString(tableOff));
        dense++;
    }

    private boolean validEntries(MemoryBlock block, String seg, int tableOff, int n) throws Exception {
        if (n < 1 || n > 512) {
            return false;
        }
        for (int i = 0; i < n; i++) {
            int v = currentProgram.getMemory().getShort(
                    toAddr(seg + ":" + String.format("%04x", tableOff + i * 2))) & 0xffff;
            Address t = toAddr(seg + ":" + String.format("%04x", v));
            if (block.getStart().compareTo(t) > 0 || block.getEnd().compareTo(t) < 0) {
                return false;
            }
        }
        return true;
    }

    private void finish(Instruction ins, ArrayList<Address> targets, String what) throws Exception {
        for (Address t : targets) {
            if (currentProgram.getListing().getInstructionAt(t) == null) {
                disassemble(t);
            }
        }
        for (Reference r : ins.getReferencesFrom()) {
            if (r.getReferenceType() == RefType.COMPUTED_JUMP) {
                removeReference(r);
            }
        }
        for (Address t : targets) {
            ins.addOperandReference(0, t, RefType.COMPUTED_JUMP, SourceType.USER_DEFINED);
        }
        Function f = currentProgram.getFunctionManager().getFunctionContaining(ins.getAddress());
        if (f != null) {
            JumpTable jt = new JumpTable(ins.getAddress(), targets, true, 0);
            jt.writeOverride(f);
            CreateFunctionCmd.fixupFunctionBody(currentProgram, f, monitor);
            println("OK " + ins.getAddress() + " " + what + " func=" + f.getName()
                    + " body=" + f.getBody().getNumAddresses());
        }
        else {
            println("OK-ORPHAN " + ins.getAddress() + " " + what
                    + " (no containing function; refs/targets only)");
        }
    }

    private int findBound(Address switchAddr) {
        Instruction cur = currentProgram.getListing().getInstructionBefore(switchAddr);
        for (int i = 0; i < 12 && cur != null; i++) {
            Matcher m = CMPAX.matcher(cur.toString());
            if (m.find()) {
                return Integer.parseInt(m.group(1).substring(2), 16);
            }
            cur = currentProgram.getListing().getInstructionBefore(cur.getAddress());
        }
        return -1;
    }
}
