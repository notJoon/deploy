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

I'll translate your text from Korean to English.

## Why Do We Sort?

Currently, this tool has the functionality to perform topological sorting based on analyzed dependencies. When deploying a single package, the order may not be an issue, but when deploying multiple packages, failure to consider dependencies can result in recognition problems after deployment is completed. For example, there may be situations where addresses declared as constants in certain contracts are not recognized.

Therefore, a sorting function is necessary to prevent such situations in advance. The sorted data is used later when generating code. This way, users don't need to worry about dependencies between packages and can simply focus on deployment.

## TODO

- codegen with maketx query format
