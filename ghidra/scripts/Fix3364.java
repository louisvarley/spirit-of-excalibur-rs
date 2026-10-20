// Convert the mislabelled 3364 initialized-data block to a data block and
// remove the four 1-byte pseudo-functions created inside BSS.
// @category AI

import java.util.ArrayList;
import java.util.List;

import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.Address;
import ghidra.program.model.address.AddressSet;
import ghidra.program.model.listing.Function;
import ghidra.program.model.listing.FunctionIterator;
import ghidra.program.model.mem.MemoryBlock;

public class Fix3364 extends GhidraScript {

    @Override
    protected void run() throws Exception {
        Address start = toAddr("3364:0000");
        MemoryBlock block = currentProgram.getMemory().getBlock(start);
        println("BEFORE block=" + block.getName() + " " + block.getStart() + "-" + block.getEnd()
                + " r=" + block.isRead() + " w=" + block.isWrite() + " x=" + block.isExecute());
        println("BEFORE totalFunctions=" + currentProgram.getFunctionManager().getFunctionCount());

        // 1. Clear code only if any exists. There are 0 instructions here; calling
        //    clearCodeUnits unconditionally also removed the 907 defined data items.
        int insCount = 0;
        ghidra.program.model.listing.InstructionIterator ii =
                currentProgram.getListing().getInstructions(
                        new AddressSet(block.getStart(), block.getEnd()), true);
        while (ii.hasNext()) {
            ii.next();
            insCount++;
        }
        println("BEFORE instructions=" + insCount);
        if (insCount > 0) {
            currentProgram.getListing().clearCodeUnits(block.getStart(), block.getEnd(), false);
        }

        // 2. Make it a normal DGROUP data block.
        block.setRead(true);
        block.setWrite(true);
        block.setExecute(false);
        block.setName("DGROUP_DATA");
        println("AFTER  block=" + block.getName() + " " + block.getStart() + "-" + block.getEnd()
                + " r=" + block.isRead() + " w=" + block.isWrite() + " x=" + block.isExecute());

        // 3. Remove any function defined inside the new data block (expected: none).
        removeFunctionsIn(block.getStart(), block.getEnd());

        // 4. Remove the four bogus 1-byte functions in the BSS block. They are
        //    relocation aliases of real code at 2d65:0001/09d4/1297/13e2.
        String[] bogus = { "3364:a011", "3364:a9e4", "3364:b2a7", "3364:b3f2" };
        for (String a : bogus) {
            Address addr = toAddr(a);
            Function f = currentProgram.getFunctionManager().getFunctionAt(addr);
            if (f != null) {
                currentProgram.getFunctionManager().removeFunction(addr);
                println("REMOVED bogus function " + a);
            }
        }

        println("AFTER  totalFunctions=" + currentProgram.getFunctionManager().getFunctionCount());
        // Headless saves the program automatically after the script returns.
    }

    private void removeFunctionsIn(Address start, Address end) {
        AddressSet range = new AddressSet(start, end);
        List<Address> entries = new ArrayList<>();
        FunctionIterator fit = currentProgram.getFunctionManager().getFunctions(true);
        while (fit.hasNext()) {
            Function f = fit.next();
            if (range.contains(f.getEntryPoint())) {
                entries.add(f.getEntryPoint());
            }
        }
        for (Address a : entries) {
            currentProgram.getFunctionManager().removeFunction(a);
            println("REMOVED function in data block " + a);
        }
    }
}
