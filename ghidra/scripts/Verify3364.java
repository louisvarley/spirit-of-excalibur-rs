// Read-only verification of the 3364 block before it is converted to data.
// @category AI

import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.Address;
import ghidra.program.model.address.AddressSet;
import ghidra.program.model.listing.Data;
import ghidra.program.model.listing.DataIterator;
import ghidra.program.model.listing.Function;
import ghidra.program.model.listing.FunctionIterator;
import ghidra.program.model.listing.Instruction;
import ghidra.program.model.listing.InstructionIterator;
import ghidra.program.model.mem.MemoryBlock;
import ghidra.program.model.symbol.Reference;
import ghidra.program.model.symbol.ReferenceIterator;

public class Verify3364 extends GhidraScript {
    @Override
    protected void run() throws Exception {
        Address start = toAddr("3364:0000");
        MemoryBlock b = currentProgram.getMemory().getBlock(start);
        println("BLOCK name=" + b.getName() + " start=" + b.getStart() + " end=" + b.getEnd()
                + " size=" + b.getSize() + " read=" + b.isRead() + " write=" + b.isWrite()
                + " exec=" + b.isExecute() + " init=" + b.isInitialized());

        AddressSet blockSet = new AddressSet(b.getStart(), b.getEnd());

        int insCount = 0;
        InstructionIterator ii = currentProgram.getListing().getInstructions(blockSet, true);
        while (ii.hasNext()) {
            Instruction ins = ii.next();
            if (insCount < 10) {
                println("INSN " + ins.getAddress() + "  " + ins);
            }
            insCount++;
        }

        int dataCount = 0;
        int stringCount = 0;
        DataIterator di = currentProgram.getListing().getDefinedData(blockSet, true);
        while (di.hasNext()) {
            Data d = di.next();
            dataCount++;
            if (d.hasStringValue()) {
                stringCount++;
            }
        }

        int fnCount = 0;
        FunctionIterator fit = currentProgram.getFunctionManager().getFunctions(true);
        while (fit.hasNext()) {
            Function f = fit.next();
            if (b.getStart().compareTo(f.getEntryPoint()) <= 0
                    && b.getEnd().compareTo(f.getEntryPoint()) >= 0) {
                println("FUNC " + f.getEntryPoint() + " " + f.getName()
                        + " bodySize=" + f.getBody().getNumAddresses()
                        + " insns=" + countInsns(f));
                fnCount++;
            }
        }

        int refCount = 0;
        FunctionIterator fit2 = currentProgram.getFunctionManager().getFunctions(true);
        while (fit2.hasNext()) {
            Function f = fit2.next();
            if (b.getStart().compareTo(f.getEntryPoint()) <= 0
                    && b.getEnd().compareTo(f.getEntryPoint()) >= 0) {
                ReferenceIterator ri = currentProgram.getReferenceManager()
                        .getReferencesTo(f.getEntryPoint());
                while (ri.hasNext()) {
                    Reference r = ri.next();
                    if (refCount < 10) {
                        println("REF to " + r.getToAddress() + " from " + r.getFromAddress()
                                + " type=" + r.getReferenceType());
                    }
                    refCount++;
                }
            }
        }

        println("SUMMARY instructions=" + insCount + " definedData=" + dataCount
                + " strings=" + stringCount + " functions=" + fnCount
                + " refsToBlock=" + refCount);
    }

    private int countInsns(Function f) {
        int n = 0;
        InstructionIterator ii = currentProgram.getListing().getInstructions(f.getBody(), true);
        while (ii.hasNext()) {
            ii.next();
            n++;
        }
        return n;
    }
}
