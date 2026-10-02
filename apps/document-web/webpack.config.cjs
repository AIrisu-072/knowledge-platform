const path = require('node:path');
const HtmlWebpackPlugin = require('html-webpack-plugin');
const MiniCssExtractPlugin = require('mini-css-extract-plugin');
const cssnano = require('cssnano');

module.exports = (_environment, argv) => {
  const production = argv.mode === 'production';
  const styleLoader = production ? MiniCssExtractPlugin.loader : 'style-loader';

  return {
    mode: production ? 'production' : 'development',
    entry: path.resolve(__dirname, 'src/main.tsx'),
    output: {
      path: path.resolve(__dirname, 'dist'),
      filename: production ? 'assets/[name].[contenthash:8].js' : 'assets/[name].js',
      publicPath: '/',
      clean: true,
    },
    resolve: {
      extensions: ['.tsx', '.ts', '.jsx', '.js'],
    },
    module: {
      rules: [
        {
          // Ajv standalone ESM also imports its qualified runtime helpers with
          // require(). Parse both forms so these calls are bundled for browsers.
          include: path.resolve(__dirname, 'src/application/search-validators.generated.js'),
          type: 'javascript/auto',
        },
        {
          test: /\.[jt]sx?$/,
          exclude: /node_modules/,
          use: 'babel-loader',
        },
        {
          test: /\.css$/i,
          use: [
            styleLoader,
            {
              loader: 'css-loader',
              options: {
                modules: {
                  auto: /\.module\.css$/i,
                  namedExport: false,
                  localIdentName: production ? '[hash:base64:6]' : '[name]__[local]',
                },
                importLoaders: 1,
              },
            },
            {
              loader: 'postcss-loader',
              options: {
                postcssOptions: {
                  plugins: production ? [cssnano({ preset: 'default' })] : [],
                },
              },
            },
          ],
        },
      ],
    },
    plugins: [
      new HtmlWebpackPlugin({ template: path.resolve(__dirname, 'index.html') }),
      ...(production ? [new MiniCssExtractPlugin({ filename: 'assets/[name].[contenthash:8].css' })] : []),
    ],
    devtool: production ? 'source-map' : 'eval-source-map',
    devServer: {
      host: '127.0.0.1',
      port: 8080,
      hot: true,
      historyApiFallback: true,
      client: { overlay: { errors: true, warnings: false } },
    },
  };
};
