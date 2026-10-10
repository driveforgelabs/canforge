# Subject: EU database/copyright question about publishing vehicle diagnostic data

Hello,

I'm building an open-source vehicle diagnostics tool, canforge, and a companion public database on GitHub, `vehicle-db`. I'm based in [Portugal / the EU, please confirm] and need legal analysis before publishing anything. Please tell me which points depend on facts you'd need to ask me about.

## Background

- DDT4ALL is a GPL-3.0 open-source tool for Renault ECU diagnostics. It reads an ECU database of requests, reply byte layouts, scaling formulas, data item names and units, and value lists.
- As far as I understand, that database comes from Renault's DDT2000 data and is not licensed by Renault for redistribution. I have not verified how it was originally obtained.
- I have a copy of the DDT4ALL database on my own machine.
- My tool can read those files from the user's own disk to decode their own vehicle's data. It does not bundle or copy them.

## What I'm considering

I want to publish an open-source database (data under CC0 or CC-BY-SA, tooling under MIT or Apache-2.0). One option I'm weighing is to take the DDT4ALL data and manually relabel every entry, renaming items and rewriting descriptions, then publish the result.

## Questions

1. Does manual relabelling of a protected database avoid infringement of the **sui generis database right** (Directive 96/9/EC) or of copyright in the selection and arrangement? Does it matter whether the relabelled entries are still structurally and substantively the same data?
2. Where is the line between unprotected technical facts (for example, "byte 3 of this reply is rail pressure, scaled by 1/10") and a protected substantial part of a database? How does the "substantial part" test and the rule on repeated, systematic extraction of insubstantial parts apply to an entry-by-entry rewrite?
3. If I instead record CAN traffic from my own car and work out the byte layouts and scaling independently, and write those up myself, is that safe to publish? Does it make a difference if I used the DDT4ALL files only to check my results, versus never opening them?
4. Is a clean-room process (one person observes the car, another writes the entries, no access to the proprietary source) a meaningful protection, and what documentation should I keep?
5. Is it lawful for my tool to read a user's locally stored DDT4ALL files and decode data with them, without redistributing them? Does the **DSM Directive** text-and-data-mining exception, the **Software Directive 2009/24/EC** interoperability exceptions, or the repair-information rules in **Regulation 715/2007** (and its successors) help or hurt here?
6. If the DDT4ALL data was itself obtained from Renault without permission, does that change my exposure for using or republishing derived data?
7. For the public database, what contributor terms and provenance fields would you recommend to reduce risk (for example a DCO sign-off, a required `source` field, a takedown process)?
8. Does the answer differ for manufacturer-specific data from other sources, such as FORScan's module data, or for data from open datasets such as Wal33D/dtc-database (MIT)?

I'm not asking you to approve anything yet. I want a summary of the risks, what is clearly safe, what is clearly not, and what I should confirm with a qualified lawyer in [jurisdiction] before publishing.

Thank you.
