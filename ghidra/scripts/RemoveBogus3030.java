// Remove the bogus 1-byte function at 1ba8:3030.
// @category AI

import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.Address;
import ghidra.program.model.listing.Function;

public class RemoveBogus3030 extends GhidraScript {
    @Override
    protected void run() throws Exception {
        Address a = toAddr("1ba8:3030");
        Function f = currentProgram.getFunctionManager().getFunctionAt(a);
        if (f != null) {
            currentProgram.getFunctionManager().removeFunction(a);
            println("REMOVED 1ba8:3030");
        }
        else {
            println("NO FUNCTION 1ba8:3030");
        }
    }
}
