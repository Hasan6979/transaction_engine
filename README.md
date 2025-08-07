##Overview##
This repository contains a small test transaction engine
simulation of a bank.

It supports Deposit, Withdrawal, Dispute, Resolve and Chargeback.

In case of Chargeback the client account is locked and no further
transactions would take place.

##Design decisions##
To handle the amounts precisely upto 4 decimal places `rust_decimal` crate
is used as opposed to floating point calculation since fp has inaccuracies
which are unacceptable in a financial instituion. `Decimal` value contains
96 bits, but those extra 32 bits are important for this kind of system.

Transactions csv file is read per line so as not to over bloat the memory.

Unit tests are written to ensure robustness, and unit tests were tested against
mutation testing to have trust in the testing system. No mutant was missed.

Tracing crate is used for logging of errors and information about the transactions.
In case of an error or warning, it is logged and the transaction is ignored and moved
on.

##Assumption##
The following assumptions were taken for this transaction engine.
 * A Dispute can only be opened for Deposit transactions
 * A dispute can only be opened if there are sufficient funds in the account
 * Available, held and total funds can never be negative