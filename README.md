# Deploy

## Dependency Analysis

The DependencyAnalyzer provides a method for analyzing dependencies between Gno packages. It calculates coupling metrics to help identify highly coupled or unstable packages, which is essential for maintaining a healthy codebase. The analyzer works by:

1. Parsing Go-like grammar source files to extract package declarations and import statements
2. Building a dependency graph between packages
3. Calculating key metrics:

$$I = C_e/(C_a+C_e)$$
   - **Afferent coupling ($C_a$)**: The number of packages that depend on a package (incoming dependencies)
   - **Efferent coupling ($C_e$)**: The number of packages a package depends on (outgoing dependencies)
   - **Instability ($I$)**: Calculated as $Ce/(Ca+Ce)$, ranging from $0$ (stable) to $1$ (unstable)

Once analyzed, you can generate deployment orders based on topological sorting, ensuring dependencies are deployed before dependent packages. The analyzer gracefully handles cyclic dependencies when they occur. Results can be exported in both JSON and text formats, with options for detailed metrics that include coupling scores and all import relationships.

## Topological Sorting for Gno Package Deployment

One of the primary motivations for implementing topological sorting in our dependency analysis tool is to address a critical issue in Gno package deployment. When deploying packages in an incorrect order (i.e., when prerequisite packages aren't deployed first), the system doesn't raise errors during the ABCI query (maketx) phase.

This silent failure can lead to complications that are difficult to debug. Our analyzer uses topological sorting to generate a proper deployment sequence, ensuring that all dependencies are satisfied before dependent packages are deployed.

The algorithm identifies packages with no dependencies first, then progressively works through the dependency chain, handling cyclic dependencies when they occur. This approach significantly reduces deployment failures and simplifies the process of managing complex package hierarchies in environments.

## TODO

- codegen with maketx query format
